//! The look itself: what she sees when she looks at a place, what she is
//! offered there, and handing it all to a brain.

use chrono::{DateTime, Utc};
use ghost_brain::{Brain, Completion, Effort, Item, Turn};
use ghost_core::{
    memory::{Person, PlaceState},
    narrate,
    prompts::Loop,
    social::PlaceKind,
    when::when,
    will::{self, Stage},
};
use ghost_work::{
    Toolbox,
    catalog::{self, Need},
};

use super::{
    Mind, Outcome,
    attend::{Looking, Thinking, Thought, Why},
};
use crate::{
    Act, Can, Error, Result,
    actions::{self, Deliberation, Offer},
};

/// The tools that bring something back and so ask for another turn. None of
/// them is offered on a look's last turn.
const BRINGS_BACK: &[&str] = &[
    "recall",
    "reread",
    "search",
    "fetch",
    "read",
    "write",
    "edit",
    "shell",
    "load_tools",
    "use_skill",
];
/// How many of the latest messages are read for words that bring back a
/// piece of her past.
const PAST_SCAN: usize = 3;

impl Mind {
    /// Hand a look at a place to a brain. The answer comes back as an outcome.
    pub(super) fn ponder(
        &mut self,
        mut job: Thinking,
        place: &PlaceState,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let brain = if job.cheap {
            self.tick.clone()
        } else {
            self.think.clone()
        };
        let with_tools = self.cfg.talk_tools && brain.works();
        let offer = self.offer(place, with_tools, &job.loaded, now)?;
        let mut tools = actions::tools(&self.prompts, &offer);
        // On the last turn of a look nothing more can come back to her, so
        // only what acts is offered, and she is told so: a look that runs out
        // of turns must still end in an answer, never in silence.
        let last = with_tools && job.round > 0 && job.round + 1 >= self.cfg.talk_steps;
        if last {
            // A tool of a server she loaded brings something back too.
            tools.retain(|t| {
                !BRINGS_BACK.contains(&t.name.as_str()) && catalog::find(&t.name).is_some()
            });
            job.items.push(Item::user(self.prompts.text("last_turn")));
        }
        // What she sees is put together once for a look made of tool
        // calls, and again for each round of one made of JSON replies.
        let request = if with_tools && !job.items.is_empty() {
            None
        } else {
            let names: Vec<&str> = if with_tools {
                tools.iter().map(|t| t.name.as_str()).collect()
            } else {
                // Answering in JSON, she can do what reply_json.md lists.
                ["undertake", "send_file"]
                    .into_iter()
                    .filter(|n| match *n {
                        "undertake" => offer.means.is_some(),
                        _ => offer.files,
                    })
                    .collect()
            };
            let abilities = self.abilities_here(&names);
            Some(self.compose(place, &mut job, now, with_tools, &abilities)?)
        };
        if with_tools {
            let spec = serde_json::to_string(&tools).unwrap_or_default();
            job.context
                .insert("tools".to_owned(), spec.chars().count() as u64);
        }
        self.budget.spend(now);
        if job.look == 0 {
            self.looks += 1;
            job.look = self.looks;
        }
        if job.round == 0
            && matches!(job.why, Why::Unread | Why::Directive(_))
            && self.can(&place.channel, Can::Read)
        {
            let upto = self
                .ghost
                .memory()
                .line(job.upto)?
                .and_then(|(_, line)| line.platform_id);
            self.fire(
                &place.channel,
                Act::Read {
                    place: place.id.clone(),
                    upto,
                },
            );
        }
        let (look, intention, kept) = (job.look, job.intention(), job.kept);
        let abort = if with_tools {
            let system = match request {
                Some((system, prompt)) => {
                    job.items.push(Item::user(prompt));
                    system
                }
                None => {
                    self.portrait_on(
                        now,
                        Some(&place.channel),
                        &self.abilities_here(
                            &tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
                        ),
                    )?
                    .0
                }
            };
            let turn = Turn {
                system,
                items: job.items.clone(),
                tools,
                max_tokens: self.cfg.max_tokens,
                temperature: None,
                // Talking is not a problem to be worked out at length, and
                // what she thinks is spent from the same allowance as what
                // she says.
                effort: Some(Effort::Low),
            };
            self.jobs.spawn(async move {
                let result = brain
                    .step(&turn)
                    .await
                    .map(Thought::Step)
                    .map_err(Error::from);
                Outcome::Thought {
                    job: Box::new(job),
                    result,
                }
            })
        } else {
            // Answering in JSON, what she sees is always put together.
            let Some((system, prompt)) = request else {
                return Ok(());
            };
            let request = Completion::ask(system, prompt, self.cfg.max_tokens)
                .expecting_json()
                .thinking(Effort::Low);
            self.jobs.spawn(async move {
                let result = deliberate(brain.as_ref(), &request)
                    .await
                    .map(Thought::Whole);
                Outcome::Thought {
                    job: Box::new(job),
                    result,
                }
            })
        };
        self.thinking.insert(
            place.key.clone(),
            Looking {
                look,
                abort,
                intention,
                kept,
            },
        );
        Ok(())
    }

    /// Put together what she sees when she looks at a place.
    /// Returns the system prompt and the prompt: who she is, and what is
    /// in front of her. `with_tools` chooses how she is told to answer.
    /// What each part of it comes to, in characters, is left on the job,
    /// for the record of her looks.
    fn compose(
        &self,
        place: &PlaceState,
        job: &mut Thinking,
        now: DateTime<Utc>,
        with_tools: bool,
        abilities: &str,
    ) -> Result<(String, String)> {
        let memory = self.ghost.memory();
        let tz = self.life.clock().tz();
        // The recent messages as they were written. Of what came before
        // them she has her own note, if the conversation is that long; then
        // everything since the note is shown, so that nothing falls between.
        let kept = memory.digest(&place.key)?;
        let mut lines = memory.thread(
            &place.key,
            self.cfg.thread_window * if kept.is_some() { 2 } else { 1 },
        )?;
        if let Some(kept) = &kept {
            lines.retain(|l| l.seq > kept.upto);
        }
        job.upto = lines
            .iter()
            .filter(|l| l.incoming)
            .map(|l| l.seq)
            .max()
            .unwrap_or(0)
            .max(place.read_upto);
        job.unread_chars = lines
            .iter()
            .filter(|l| l.incoming && l.seq > place.read_upto)
            .map(|l| l.text.chars().count())
            .sum();

        let mut people = memory.people_in(&place.key, 6)?;
        if people.is_empty()
            && let Some(person) = memory.person_of_direct(&place.key)?
        {
            people.push(person);
        }
        let mut cards = Vec::with_capacity(people.len());
        for person in &people {
            let standing = will::standing(person, now);
            let (account, notes) = self.known_of(person, place, 8, now)?;
            let mut card = narrate::person_card(person, &standing, &[], now, self.life.clock());
            card.push_str(&account);
            for note in &notes {
                card.push_str(&format!("\n  · {}", note.text));
            }
            cards.push(card);
        }
        // What she already means to do here, so she does not resolve to do it twice.
        let keys: Vec<String> = people.iter().map(|p| p.key.clone()).collect();
        let pursuing = match &job.why {
            Why::Intention(intention) => Some(intention.seq),
            Why::Unread | Why::Directive(_) => None,
        };
        let meant: Vec<String> = self
            .intentions_about(&place.key, &keys)?
            .into_iter()
            .filter(|i| Some(i.seq) != pursuing)
            .map(|i| {
                format!(
                    "- by {}: {}",
                    i.due.with_timezone(&tz).format("%m-%d %H:%M"),
                    i.what
                )
            })
            .collect();
        if !meant.is_empty() {
            cards.push(format!("\nYou already mean to:\n{}", meant.join("\n")));
        }

        let addressed = match (place.kind, place.unread_addressed > 0) {
            (PlaceKind::Direct, _) => self.prompts.text("addressed_direct").to_owned(),
            (_, true) => self.prompts.text("addressed_you").to_owned(),
            (_, false) => self.unaddressed(place, now)?,
        };
        let why = match &job.why {
            Why::Unread => match place.unread {
                1 => "There is 1 new message here since you last looked.".to_owned(),
                n => format!("There are {n} new messages here since you last looked."),
            },
            Why::Intention(intention) if intention.report => {
                self.report_brief(intention, place, now)?
            }
            Why::Intention(intention) => self
                .prompts
                .fill("intention_due", &[("what", intention.what.clone())]),
            Why::Directive(directives) => self
                .prompts
                .fill("directive", &[("directives", self.directives(directives))]),
        };
        let recalled = self.prompts.section(
            "recalled",
            "recalled",
            job.recalled.as_deref().unwrap_or_default(),
        );

        // The pieces of her past that the last few messages touch, and only
        // those: what is in front of her every time is what she talks about.
        let recent: Vec<&str> = lines
            .iter()
            .rev()
            .take(PAST_SCAN)
            .map(|l| l.text.as_str())
            .collect();
        let touched: Vec<&ghost_core::identity::Past> = self
            .ghost
            .identity()
            .seed()
            .past
            .iter()
            .filter(|p| p.touched_by(&recent))
            .collect();
        let brought_back = self
            .prompts
            .section("past", "past", &narrate::past(&touched));

        let stopped_here = self.stopped_here(place, &lines, now)?;
        let (who, right_now) = self.portrait_on(now, Some(&place.channel), abilities)?;
        let reply = if with_tools {
            self.prompts.reply_tools.clone()
        } else {
            narrate::fill(
                &self.prompts.reply_json,
                &[(
                    "abilities",
                    self.abilities(
                        self.can(&place.channel, Can::Files),
                        self.has_work_under_way()?,
                        place.kind == PlaceKind::Group,
                    ),
                )],
            )
        };
        let people_text = if cards.is_empty() {
            "(nobody you know yet)".to_owned()
        } else {
            cards.join("\n")
        };
        let transcript = format!(
            "{}{}{}",
            kept.map_or_else(String::new, |d| format!(
                "What you kept of this conversation{} before the messages below:\n{}\n\n",
                d.sources.map_or_else(String::new, |s| {
                    let clock = self.life.clock();
                    format!(
                        ", from {} until {},",
                        when(s.since, now, clock),
                        when(s.until, now, clock)
                    )
                }),
                d.text
            )),
            // A directive is marked in its line's heading,
            // where nothing anyone types can reach, so that she
            // can tell it from what people say to her.
            narrate::transcript(
                &lines,
                place.read_upto,
                self.prompts.text("directive_mark"),
                self.life.clock(),
                now
            ),
            // Being stopped here goes with the conversation it
            // happened in, for as long as it is in front of her.
            stopped_here
        );
        let chars = |s: &str| s.chars().count() as u64;
        let parts = [
            ("system", chars(&who)),
            ("people", chars(&people_text)),
            ("transcript", chars(&transcript)),
            ("recalled", chars(&recalled)),
            ("past", chars(&brought_back)),
        ];
        let prompt = narrate::fill(
            &self.prompts.attend,
            &[
                ("where", self.describe_place(place, &people)),
                ("elsewhere", self.elsewhere(place, now)?),
                ("now", right_now),
                ("right_now", self.right_now(place, &lines, now)?),
                ("reply", reply),
                ("people", people_text),
                ("transcript", transcript),
                ("recalled", recalled),
                ("brought_back", brought_back),
                ("why", why),
                ("addressed", addressed),
            ],
        );
        // The rest of the prompt: its frame, and what is said in it of
        // where she is and why she looks.
        let inside: u64 = parts[1..].iter().map(|(_, n)| n).sum();
        for (name, n) in parts {
            job.context.insert(name.to_owned(), n);
        }
        job.context
            .insert("frame".to_owned(), chars(&prompt).saturating_sub(inside));
        Ok((who, prompt))
    }

    pub(super) fn describe_place(&self, place: &PlaceState, people: &[Person]) -> String {
        let channel = ("channel", place.channel.clone());
        let mut out = match place.kind {
            PlaceKind::Direct => self.prompts.fill(
                "place_direct",
                &[
                    channel,
                    (
                        "who",
                        people
                            .first()
                            .map_or("someone", |p| p.name.as_str())
                            .to_owned(),
                    ),
                ],
            ),
            PlaceKind::Group => self.prompts.fill(
                "place_group",
                &[
                    channel,
                    (
                        "name",
                        place
                            .name
                            .as_deref()
                            .map_or_else(|| "(unnamed)".to_owned(), |n| format!("\"{n}\"")),
                    ),
                ],
            ),
            PlaceKind::Public => self.prompts.fill("place_public", &[channel]),
        };
        if let Some(me) = self
            .channels
            .get(&place.channel)
            .and_then(|c| c.me.as_ref())
        {
            out.push_str(
                &self.prompts.fill(
                    "place_me",
                    &[(
                        "me",
                        me.handle
                            .as_deref()
                            .map_or_else(|| me.name.clone(), |h| format!("@{h}")),
                    )],
                ),
            );
        }
        out
    }

    /// What taking things on would be done with, if she can at all.
    pub(super) fn work_means(&self) -> Option<&str> {
        self.can_work().then(|| {
            self.prompts.text(if self.workshop.machine.is_some() {
                "means_machine"
            } else {
                "means_desk"
            })
        })
    }

    /// Whether she has work under way that she could steer or stop.
    fn has_work_under_way(&self) -> Result<bool> {
        Ok(!self.ghost.memory().undertakings(true, 1)?.is_empty())
    }

    /// What a look at `place` offers beyond what is always there. With
    /// `with_tools`, that includes what reaches beyond the conversation, as
    /// it is said there: what it is, and the conversation's pace.
    fn offer(
        &self,
        place: &PlaceState,
        with_tools: bool,
        loaded: &ghost_work::tools::Loaded,
        now: DateTime<Utc>,
    ) -> Result<Offer> {
        let mut offer = Offer {
            means: self.work_means().map(str::to_owned),
            files: self.can(&place.channel, Can::Files),
            under_way: self.has_work_under_way()?,
            group: place.kind == PlaceKind::Group,
            reaching: Vec::new(),
        };
        if with_tools {
            let glance = self.prompts.text("at_a_glance");
            offer.reaching = self
                .reaching(place, loaded, now)?
                .specs(self.prompts.store(), Loop::Conversation)
                .into_iter()
                .map(|mut spec| {
                    spec.description = format!("{} {glance}", spec.description);
                    spec
                })
                .collect();
        }
        Ok(offer)
    }

    /// The tools that reach beyond a conversation, as she has them in
    /// `place`: the web, if she has it; her machine, if she has one and
    /// may use it here; the servers of tools she has, with the tools of
    /// those `loaded` in this look; and her skills.
    pub(super) fn reaching(
        &self,
        place: &PlaceState,
        loaded: &ghost_work::tools::Loaded,
        now: DateTime<Utc>,
    ) -> Result<Toolbox> {
        let web = self.cfg.work.web.then_some(&self.workshop.web);
        let machine = if self.machine_here(place, now)? {
            self.workshop.machine.clone()
        } else {
            None
        };
        let tools = ghost_work::tools::reaching(web, machine);
        let tools = match &self.workshop.servers {
            Some(servers) => ghost_work::tools::with_servers(tools, servers, loaded, false),
            None => tools,
        };
        Ok(match &self.workshop.skills {
            Some(skills) => ghost_work::tools::with_skills(tools, skills, false),
            None => tools,
        })
    }

    /// Whether she may use her machine in the middle of this conversation.
    /// Nobody she does not know well gets her hands on a shell, here any
    /// more than at work: only in private, with a friend or closer. Those
    /// who run her body are not held to that: where one of them has given
    /// her a directive, or in private with one of them, it is there.
    fn machine_here(&self, place: &PlaceState, now: DateTime<Utc>) -> Result<bool> {
        if !self.cfg.talk_machine || self.workshop.machine.is_none() {
            return Ok(false);
        }
        if place.unread_directed > 0 {
            return Ok(true);
        }
        if place.kind != PlaceKind::Direct {
            return Ok(false);
        }
        Ok(self
            .ghost
            .memory()
            .person_of_direct(&place.key)?
            .is_some_and(|p| {
                self.cfg.operators.includes(&p.channel, &p.id)
                    || will::standing(&p, now).stage >= Stage::Friend
            }))
    }

    /// What she is told she can do here, from the tools offered here that
    /// say what having them lets her do; nothing at all when none do.
    /// Where she can take something on, the tools only work has are told
    /// too: someone may want what she cannot do yet, such as a browser,
    /// and finding one is a piece of work she can take on.
    pub(super) fn abilities_here(&self, offered: &[&str]) -> String {
        let at_work = offered.contains(&"undertake");
        let lines: Vec<String> = catalog::TOOLS
            .iter()
            .filter(|e| {
                offered.contains(&e.name)
                    || (at_work
                        && !e.loops.contains(&Loop::Conversation)
                        && e.needs.iter().all(|n| self.at_work_has(*n)))
            })
            .filter_map(|e| self.prompts.store().ability(e.name))
            .map(|a| {
                self.prompts
                    .fill("ability_line", &[("ability", a.to_owned())])
            })
            .collect();
        if lines.is_empty() {
            String::new()
        } else {
            self.prompts
                .fill("abilities", &[("abilities", lines.join("\n"))])
        }
    }

    /// Whether work she took on would have what `need` asks for. Only
    /// what a tool of work alone could need is answered; what a piece of
    /// work has depends on the piece, and is not claimed here.
    fn at_work_has(&self, need: Need) -> bool {
        match need {
            Need::Registry => self.workshop.servers.as_ref().is_some_and(|s| s.installs()),
            Need::SkillIndex => self.workshop.skills.as_ref().is_some_and(|s| s.installs()),
            _ => false,
        }
    }

    /// Directives as she is shown them: one to a line.
    pub(super) fn directives(&self, directives: &[String]) -> String {
        directives
            .iter()
            .map(|d| self.prompts.fill("directive_item", &[("text", d.clone())]))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

async fn deliberate(brain: &dyn Brain, request: &Completion) -> Result<Deliberation> {
    let reply = brain.complete(request).await?;
    actions::parse(&reply.text).map_err(Error::Parse)
}
