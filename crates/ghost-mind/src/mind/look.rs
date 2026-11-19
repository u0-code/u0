//! The look itself: what she sees when she looks at a place, what she is
//! offered there, and handing it all to a brain.

use chrono::{DateTime, Utc};
use ghost_brain::{Brain, Completion, Effort, Item, Turn};
use ghost_core::{memory::PlaceState, narrate, prompts::Loop, social::PlaceKind, when::when, will};
use ghost_work::Toolbox;

use super::{
    Mind, Outcome, Purpose,
    attend::{Looking, Thinking, Thought, Why},
};
use crate::{
    Act, Can, Error, Result,
    actions::{self, Deliberation, Offer},
    catalog::{self, Kind, Need},
};

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
        let offer = self.offer(place, with_tools, &job.loaded)?;
        let mut tools = actions::tools(&self.prompts, &offer);
        // On the last turn of a look nothing more can come back to her, so
        // only what acts is offered, and she is told so: a look that runs out
        // of turns must still end in an answer, never in silence.
        let last = with_tools && job.round > 0 && job.round + 1 >= self.cfg.talk_steps;
        if last {
            tools.retain(|t| !catalog::brings_back(&t.name));
            job.items.push(Item::user(self.prompts.text("last_turn")));
        }
        // What she sees is put together once for a look made of tool
        // calls, and again for each round of one made of JSON replies.
        let request = if with_tools && !job.items.is_empty() {
            None
        } else {
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            let abilities = self.abilities_here(&names);
            // Answering in JSON, she is told the same things as a list.
            let actions = if with_tools {
                String::new()
            } else {
                actions::listing(&tools)
            };
            Some(self.compose(place, &mut job, now, with_tools, &abilities, &actions)?)
        };
        if with_tools {
            let spec = serde_json::to_string(&tools).unwrap_or_default();
            job.context
                .insert("tools".to_owned(), spec.chars().count() as u64);
        }
        self.budget.spend(now);
        if job.round == 0 && matches!(job.why, Why::Unread) && self.can(&place.channel, Can::Read) {
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
        let purpose = Purpose::Look {
            place: place.key.clone(),
            looking: Looking {
                intention: job.intention(),
                kept: job.kept,
            },
        };
        if with_tools {
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
            self.jobs.spawn_for(purpose, async move {
                let result = brain
                    .step(&turn)
                    .await
                    .map(Thought::Step)
                    .map_err(Error::from);
                Outcome::Thought {
                    job: Box::new(job),
                    result,
                }
            });
        } else {
            // Answering in JSON, what she sees is always put together.
            let Some((system, prompt)) = request else {
                return Ok(());
            };
            let request = Completion::ask(system, prompt, self.cfg.max_tokens)
                .expecting_json()
                .thinking(Effort::Low);
            self.jobs.spawn_for(purpose, async move {
                let result = deliberate(brain.as_ref(), &request)
                    .await
                    .map(Thought::Whole);
                Outcome::Thought {
                    job: Box::new(job),
                    result,
                }
            });
        }
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
        actions: &str,
    ) -> Result<(String, String)> {
        let memory = self.ghost.memory();
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
            let mut card = crate::narrate::person_card(
                &self.prompts,
                person,
                &standing,
                &[],
                now,
                self.life.clock(),
            );
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
            Why::Unread => None,
        };
        let meant: Vec<_> = self
            .intentions_about(&place.key, &keys)?
            .into_iter()
            .filter(|i| Some(i.seq) != pursuing)
            .collect();
        let meant = self
            .prompts
            .section("meant", "lines", &self.meant_lines(&meant));
        if !meant.is_empty() {
            cards.push(meant);
        }

        let addressed = match (place.said_to_her(), place.kind) {
            (false, _) => self.unaddressed(place, now)?,
            (true, PlaceKind::Direct) => self.prompts.text("addressed_direct").to_owned(),
            (true, _) => self.prompts.text("addressed_you").to_owned(),
        };
        let why = match &job.why {
            Why::Unread => match place.unread {
                1 => self.prompts.text("unread_one").to_owned(),
                n => self
                    .prompts
                    .fill("unread_many", &[("count", n.to_string())]),
            },
            Why::Intention(intention) if intention.report => {
                self.report_brief(intention, place, now)?
            }
            Why::Intention(intention) => self
                .prompts
                .fill("intention_due", &[("what", intention.what.clone())]),
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
            narrate::fill(&self.prompts.reply_json, &[("actions", actions.to_owned())])
        };
        let people_text = self.prompts.or(cards.join("\n"), "nobody_known");
        let transcript = format!(
            "{}{}{}",
            kept.map_or_else(String::new, |d| self.prompts.fill(
                "kept_before",
                &[
                    (
                        "span",
                        d.sources.map_or_else(String::new, |s| {
                            let clock = self.life.clock();
                            self.prompts.fill(
                                "kept_span",
                                &[
                                    ("since", when(s.since, now, clock)),
                                    ("until", when(s.until, now, clock)),
                                ],
                            )
                        })
                    ),
                    ("text", d.text),
                ]
            )),
            crate::narrate::transcript(
                &self.prompts,
                &lines,
                place.read_upto,
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
        // Every channel is a place of its own, read on a page of its own.
        let page = self.cfg.page(&place.channel);
        let prompt = narrate::fill(
            self.prompts.page(&page),
            &[
                ("platform", self.prompts.platform(&page).to_owned()),
                (
                    "where",
                    self.place_words(place, &people, true)? + &self.allowed_here(place),
                ),
                ("elsewhere", self.elsewhere(place, &people, now)?),
                ("now", right_now),
                ("right_now", self.right_now(place, &lines, &people, now)?),
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

    /// What the platform does not allow her in `place`, said after where
    /// she is; nothing where it allows all a person may do.
    fn allowed_here(&self, place: &PlaceState) -> String {
        let facts = self.facts(&place.channel);
        let mut out = String::new();
        if let Some(within) = facts.answer_within(place.kind) {
            let minutes = (within.as_secs() / 60).max(1);
            out.push_str(
                &self
                    .prompts
                    .fill("place_answer_within", &[("minutes", minutes.to_string())]),
            );
        }
        if !facts.writes_first {
            out.push_str(self.prompts.text("place_not_first"));
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
    ) -> Result<Offer> {
        let here = [
            (Need::Files, self.can(&place.channel, Can::Files)),
            (Need::UnderWay, self.has_work_under_way()?),
            (Need::Group, place.kind == PlaceKind::Group),
            (Need::Social, !self.facts(&place.channel).waited_on),
            (Need::React, self.can(&place.channel, Can::React)),
            (Need::WritesFirst, self.facts(&place.channel).writes_first),
        ]
        .into_iter()
        .filter_map(|(need, had)| had.then_some(need))
        .collect();
        let mut offer = Offer {
            means: self.work_means().map(str::to_owned),
            here,
            reaching: Vec::new(),
        };
        if with_tools {
            let glance = self.prompts.text("at_a_glance");
            offer.reaching = self
                .reaching(loaded)
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

    /// The tools that reach beyond a conversation, as she has them: the
    /// web, if she has it; the servers of tools she has, with
    /// the tools of those `loaded` in this look; and her skills. Never her
    /// machine: what runs or changes something is work.
    pub(super) fn reaching(&self, loaded: &ghost_work::tools::Loaded) -> Toolbox {
        let web = self.cfg.work.web.then_some(&self.workshop.web);
        let tools = ghost_work::tools::reaching(web);
        let tools = match &self.workshop.servers {
            Some(servers) => ghost_work::tools::with_servers(tools, servers, loaded, false),
            None => tools,
        };
        match &self.workshop.skills {
            Some(skills) => ghost_work::tools::with_skills(tools, skills, false),
            None => tools,
        }
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
                        && matches!(e.kind, Kind::Work)
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
}

async fn deliberate(brain: &dyn Brain, request: &Completion) -> Result<Deliberation> {
    let reply = brain.complete(request).await?;
    actions::parse(&reply.text).map_err(Error::Parse)
}
