//! Doing things.
//!
//! When she decides to take something on, it becomes an undertaking: a
//! directory with a brief, a plan and notes, and an entry in her journal.
//! It is worked on in sittings. A sitting is a bounded run of the loop in
//! `ghost-work`, sent off as a job like any brain call, so the actor stays
//! the only writer of her journal and stays free to notice everything else.
//!
//! Work runs on her machine, apart from her day and her attention. A
//! sitting starts while she is asleep, talking or reading; a conversation
//! does not wait for work, and work does not wait for a conversation. A
//! sitting that ends because it ran out of steps or time is followed at
//! once by the next sitting of the same work, up to `max_sittings`. What
//! she is doing in her day stays what it was: the work is the machine's,
//! not something she sits at.
//!
//! Up to `at_once` pieces of work have a sitting running at
//! the same time. When a place is free, what was asked for at a terminal goes
//! first, then whatever has waited longest since it was last touched,
//! leaving aside what waits on somebody's answer and what is resting.
//!
//! Starting a sitting takes one call from the body's hourly budget, and
//! none starts while that budget is spent. The steps inside a sitting are
//! not counted one by one: they are bounded by the sitting's own steps and
//! minutes, and counting them against the hour would let a long piece of
//! work leave her nothing to answer anyone with. What the budget bounds is
//! how many sittings start in an hour, and so, with those limits, what
//! work can spend.
//!
//! The work and she talk while it runs, and only through its record in
//! `work.db`, which both the worker here and the tool server a command-line
//! agent starts can write. The work leaves there what it chose to tell her
//! and what it kept; she glances at every running sitting every so often,
//! and once more when each ends, and writes into her journal what she has
//! not had yet. Something she could hand over makes her mean to go back to
//! whoever asked; like anything she means to do, that waits for her to be
//! awake and free, unless it was asked for at a terminal. Nothing of what
//! it did to get there reaches her. She speaks to it the same way: what
//! she adds goes into its inbox, taken before its next step, and stopping
//! it is an outcome written into its record, which it sees before its next
//! step too.
//!
//! The work may hand parts of itself to other workers that run alongside
//! it during a sitting. They report to the work, through the same inbox,
//! and never to her: she sees each only as a line under the work in the
//! rest of her day.
//!
//! What is whose. Her journal is her record of the work: that she took it
//! on, for whom, where and in what folder, each sitting she gave it, what
//! she asked and was answered, what it told her and what she kept from it,
//! and that it ended and how. Its record in `work.db` is the doing: the
//! plan, the notes, the transcript, the history and the step log, the
//! inbox, the running totals, and the worker's own account of how it
//! ended. Where both speak of one thing, her journal is what she goes by,
//! and it is written second: the record is changed first (a word passed
//! on, an end), and then her journal says so. So the record is never
//! behind her journal, only ever ahead of it, and only when she stopped
//! between the two. Work her journal has open whose record has ended is a
//! sitting, or a stop, whose end she never wrote down: before she sits
//! down to anything she writes it, from the record and as the record has
//! it. Work her journal has ended is over, whatever its record says.
//!
//! She only reads the records she looks at, through one read-only handle
//! on the database: looking at work never makes a record, writes one, or
//! waits on a sitting for a lock.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use chrono::{DateTime, Duration, Utc};
use ghost_core::{
    journal::Event,
    memory::{Person, PlaceState},
    will::{self, Stage},
};
use ghost_work::{
    Ended, Limits, Report, Undertaking, WORK_DB, Worker, tools,
    undertaking::{self, Kept, Look, Records, State},
};

/// How often she glances at what a sitting in progress has told her.
const GLANCE: Duration = Duration::seconds(30);
use tracing::{info, warn};

use super::{Mind, Operator, Order, Outcome, Refused};
use crate::{Result, actions::clip};

/// A sitting in progress.
pub(super) struct Working {
    /// Set to stop the sitting after the step it is on.
    pub pause: Arc<AtomicBool>,
    /// Its record, which the sitting writes to as it goes.
    pub record: Arc<Undertaking>,
    /// The name of an operator who stopped it from a console, if one did,
    /// for when the sitting comes back stopped.
    pub stopped_by: Option<String>,
}

impl Mind {
    /// The brain her own loop steps work with, if this body has one that
    /// can take tools. A command-line agent cannot: it does parts of work
    /// it is handed, with its own loop, and never steps hers.
    fn working_brain(&self) -> Option<Arc<dyn ghost_brain::Brain>> {
        [&self.think, &self.tick]
            .into_iter()
            .find(|b| b.works())
            .cloned()
    }

    /// The undertaking `id` in her work database, to write to, started
    /// from `brief` if it has not been opened before. Only what changes the
    /// work opens it this way; looking at it goes through [`Mind::records`].
    pub(super) fn undertaking(&self, id: &str, brief: &str) -> std::io::Result<Undertaking> {
        Undertaking::open(&self.ghost.dir().join(WORK_DB), id, Some(brief))
    }

    /// Her work database, read-only, once there is one.
    pub(super) fn records(&self) -> Option<&Records> {
        if let Some(records) = self.records.get() {
            return Some(records);
        }
        let db = self.ghost.dir().join(WORK_DB);
        // A reader does not make the database: until work has been sat
        // down to, there is nothing to read.
        if !db.exists() {
            return None;
        }
        match Records::open(&db) {
            Ok(records) => Some(self.records.get_or_init(|| records)),
            Err(e) => {
                warn!("could not read {}: {e}", db.display());
                None
            }
        }
    }

    /// The records of the work `ids` as they are now, each with its parts,
    /// in that order. Work with no record, or none that can be read, is
    /// left out.
    pub(super) fn records_of(&self, ids: &[String]) -> Vec<Look> {
        let Some(records) = self.records() else {
            return Vec::new();
        };
        records.look(ids).unwrap_or_else(|e| {
            warn!("could not read the records of her work: {e}");
            Vec::new()
        })
    }

    /// Whether `person` is an account of whoever runs her: anyone at a
    /// console, or an account listed under `[operators]`.
    pub(super) fn runs_her(&self, person: &Person) -> bool {
        ghost_core::social::split_key(&person.key)
            .is_some_and(|(channel, id)| self.cfg.operators.includes(channel, id))
    }

    /// Whether taking things on is possible at all in this body.
    pub(super) fn can_work(&self) -> bool {
        self.cfg.work.enabled && self.working_brain().is_some()
    }

    /// She decided to take something on. Returns why not, if it is refused.
    /// Asked for at a terminal, it is a piece of work of its own, whatever
    /// she has already taken on today or for whom, and it goes first. Asked
    /// for by an account of whoever runs her, on any channel, it is not held
    /// to the floors her will keeps for everyone else; it is weighed and
    /// paced like anything else she is asked in a chat.
    pub(super) fn undertake(
        &mut self,
        brief: &str,
        asked_by: Option<&Person>,
        place: Option<&PlaceState>,
        dir: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Option<String>> {
        let brief = brief.trim();
        if brief.is_empty() {
            return Ok(Some("there was nothing to take on".into()));
        }
        if !self.can_work() {
            return Ok(Some("this body has nothing to work with".into()));
        }
        // Asked at a terminal, it is not hers to weigh: whoever runs her
        // asked for it, and is waiting.
        let terminal = place.is_some_and(|p| self.facts(&p.channel).waited_on);
        // Whoever runs her is let past the floors anywhere: it is their
        // machine and their account she would be spending.
        let theirs = terminal || asked_by.is_some_and(|p| self.runs_her(p));
        let memory = self.ghost.memory();
        // More about something she is already doing for the same person is
        // added to it, not started as a second thing.
        if !terminal
            && let Some(person) = asked_by
            && let Some(open) = memory
                .undertakings(true, 20)?
                .into_iter()
                .find(|u| u.asked_by.as_deref() == Some(person.key.as_str()))
        {
            let added = format!(
                "{brief}{}",
                match place {
                    Some(place) => self.their_words(place, person)?,
                    None => String::new(),
                }
            );
            return Ok(
                match self
                    .undertaking(&open.id, &open.brief)
                    .and_then(|u| u.add(&added))
                {
                    Ok(()) => {
                        info!("added to {}: {}", open.id, clip(brief, 80));
                        self.record(Event::Added {
                            id: open.id,
                            text: added,
                            by: None,
                        })?;
                        self.work_at(now);
                        None
                    }
                    Err(e) => Some(format!(
                        "it could not be added to what she is already doing: {e}"
                    )),
                },
            );
        }
        // How much she takes on, and for whom, is hers to decide, except
        // for whoever runs her.
        if !theirs
            && memory.undertaken_since(now - Duration::hours(24))?
                >= i64::from(self.cfg.work.per_day)
        {
            return Ok(Some("she has taken on enough for one day".into()));
        }
        // Her will decides what she agrees to. This is only the floor under
        // it: a stranger does not get her hands on a shell.
        if !theirs && let Some(person) = asked_by {
            let stage = will::standing(person, now).stage;
            let needed = if self.workshop.machine.is_some() {
                Stage::Friend
            } else {
                Stage::Stranger
            };
            if stage < needed {
                return Ok(Some(format!(
                    "{} is {} to her, which is not enough for this",
                    person.name,
                    crate::narrate::stage(&self.prompts, stage)
                )));
            }
        }
        let id = format!("{}-{:04x}", now.format("%Y%m%d-%H%M%S"), self.rng.u16(..));
        let dir = self.work_folder(&id, brief, dir, now);
        // Her own account of what is wanted, and under it what they said.
        let brief = format!(
            "{brief}{}",
            match (place, asked_by) {
                (Some(place), Some(person)) => self.their_words(place, person)?,
                _ => String::new(),
            },
        );
        self.record(Event::Undertook {
            id,
            brief,
            asked_by: asked_by.map(|p| p.key.clone()),
            place: place.map(|p| p.key.clone()),
            directed: terminal,
            dir,
        })?;
        self.work_at(now);
        Ok(None)
    }

    /// The folder the work `id` is done in: the one whoever asked named,
    /// or else a folder of its own under her folders, by the day it was
    /// taken on and what it is, `2026-11-18/count-the-lines-d2a4`. None
    /// where her machine is not this computer.
    fn work_folder(
        &self,
        id: &str,
        brief: &str,
        named: Option<&str>,
        now: DateTime<Utc>,
    ) -> Option<String> {
        let root = self.workshop.folders.as_ref()?;
        if let Some(named) = named.map(str::trim).filter(|d| !d.is_empty()) {
            return Some(match named.strip_prefix("~/") {
                Some(rest) => std::env::var_os("HOME").map_or_else(
                    || named.to_owned(),
                    |h| std::path::Path::new(&h).join(rest).display().to_string(),
                ),
                None => named.to_owned(),
            });
        }
        let tail = id.rsplit('-').next().unwrap_or(id);
        let name = format!("{}-{tail}", folder_name(brief));
        Some(
            root.join(now.format("%Y-%m-%d").to_string())
                .join(name)
                .display()
                .to_string(),
        )
    }

    /// Time to look at her work: glance at every sitting in progress, and
    /// sit down to whatever else can be.
    pub(super) fn work(&mut self, now: DateTime<Utc>) -> Result<()> {
        self.agenda.work = None;
        // At sittings already, this is when she glances at what each has
        // told her so far.
        let running: Vec<(String, Arc<Undertaking>)> = self
            .working
            .iter()
            .map(|(id, w)| (id.clone(), w.record.clone()))
            .collect();
        for (id, record) in &running {
            self.hear_from_work(id, &record.state(), now)?;
        }
        if !running.is_empty() {
            self.work_at(now + GLANCE);
        }
        self.resting.retain(|_, until| *until > now);
        let at_once = usize::try_from(self.cfg.work.at_once.max(1)).unwrap_or(usize::MAX);
        while self.working.len() < at_once && self.sit_down(now)? {}
        Ok(())
    }

    /// Look at her work again no later than `at`.
    fn work_at(&mut self, at: DateTime<Utc>) {
        self.agenda.work = Some(self.agenda.work.map_or(at, |due| due.min(at)));
    }

    /// Sit down to the next piece of work, if one can be. Returns whether
    /// anything changed that makes it worth looking for another.
    fn sit_down(&mut self, now: DateTime<Utc>) -> Result<bool> {
        let Some(brain) = self.working_brain().filter(|_| self.cfg.work.enabled) else {
            return Ok(false);
        };
        let open = self.ghost.memory().undertakings(true, 20)?;
        if self.settle_ended(&open, now)? {
            return Ok(true);
        }
        let patience = Duration::hours(i64::from(self.cfg.work.ask_hours));
        let unanswered = open
            .iter()
            .find(|u| u.asked_at.is_some_and(|at| now - at >= patience))
            .cloned();
        if let Some(stale) = unanswered {
            // Nobody answered. She goes on without, and says so in the work.
            let said = self.prompts.text("no_answer").to_owned();
            if let Err(e) = self
                .undertaking(&stale.id, &stale.brief)
                .and_then(|u| u.add(&said))
            {
                warn!("cannot go on with {} unanswered: {e}", stale.id);
            }
            self.record(Event::Added {
                id: stale.id,
                text: said,
                by: None,
            })?;
            return Ok(true);
        }
        // What is waiting to be answered is come back to when the first of
        // it has waited long enough.
        if let Some(give_up_at) = open
            .iter()
            .filter_map(|u| u.asked_at)
            .min()
            .map(|at| at + patience)
        {
            self.work_at(give_up_at);
        }
        // Of the rest, what is not running already, nor resting after a
        // sitting.
        let mut ready = Vec::new();
        for u in open {
            if u.waiting.is_some() || self.working.contains_key(&u.id) {
                continue;
            }
            match self.resting.get(&u.id) {
                Some(&until) => self.work_at(until),
                None => ready.push(u),
            }
        }
        // What was asked for at a terminal goes first; then whatever has waited
        // longest since it was last touched.
        let Some(next) = ready
            .into_iter()
            .min_by_key(|u| (!u.directed, u.last_at, u.seq))
        else {
            return Ok(false);
        };
        // As many sittings as one thing is given, and not done: it comes
        // back to her unfinished, to decide what to do with, not let go
        // without a word.
        if next.sittings >= i64::from(self.cfg.work.max_sittings) {
            let summary = self.prompts.fill(
                "out_of_sittings",
                &[
                    ("sittings", next.sittings.to_string()),
                    ("progress", next.summary.clone().unwrap_or_default()),
                ],
            );
            // Its record ends first, so that nothing goes on with it: a
            // tool server still holding it refuses from now on.
            if let Err(e) = self
                .undertaking(&next.id, &next.brief)
                .and_then(|record| record.end(undertaking::Outcome::Blocked, &summary))
            {
                warn!("the record of {} could not be ended: {e}", next.id);
            }
            self.end_work(&next.id, "blocked", &summary, None, &[], now)?;
            return Ok(true);
        }
        // A sitting is one call from the body's budget: its steps are
        // bounded by the sitting's own limits, and the budget bounds how
        // many sittings start.
        if !self.budget.allow(now) {
            warn!("brain budget for this hour is spent; work waits");
            self.work_at(now + Duration::minutes(10));
            return Ok(false);
        }
        let undertaking = match self.undertaking(&next.id, &next.brief) {
            Ok(u) => Arc::new(u),
            Err(e) => {
                warn!("cannot open the undertaking {}: {e}", next.id);
                self.record(Event::Concluded {
                    id: next.id,
                    outcome: "blocked".into(),
                    summary: self
                        .prompts
                        .fill("record_unopened", &[("why", e.to_string())]),
                })?;
                return Ok(true);
            }
        };
        self.budget.spend(now);
        self.sit(&next, undertaking, brain, now)?;
        Ok(true)
    }

    /// Start a sitting on `next`, whose record is `undertaking`.
    fn sit(
        &mut self,
        next: &ghost_core::memory::Undertaking,
        undertaking: Arc<Undertaking>,
        brain: Arc<dyn ghost_brain::Brain>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let before = undertaking.state().steps;
        // Done in its own folder, where her machine can be given one: the
        // record keeps where, for every sitting and every part of it.
        let folder = next.dir.clone().filter(|_| self.workshop.folders.is_some());
        if let Some(dir) = &folder
            && let Err(e) = undertaking.update(|state| {
                state.dir.get_or_insert_with(|| dir.clone());
            })
        {
            // Not done anywhere but where it is to be done.
            warn!("cannot sit down to {}: {e}", next.id);
            self.rest(&next.id, now + Duration::minutes(10));
            return Ok(());
        }
        let place = match &self.workshop.machine {
            Some(machine) => in_folder(machine, &undertaking).describe(self.prompts.store()),
            None => self.prompts.text("no_machine").to_owned(),
        };
        // What the work is told comes from the same files as what she is
        // told, and is looked at again before each step.
        let words = self.prompts.for_work();
        let shared = words.clone().shared();
        // What she works with, for this and for any part of it she hands
        // to a helper: her machine if she has one, the web, servers of
        // tools to find, install and load, each sitting loading its own,
        // and skills to find, install and read.
        let equip: ghost_work::worker::ToolsFor = {
            let (machine, web) = (self.workshop.machine.clone(), self.workshop.web.clone());
            let servers = self.workshop.servers.clone();
            let skills = self.workshop.skills.clone();
            Arc::new(move |undertaking| {
                let tools = match &machine {
                    Some(machine) => tools::standard(in_folder(machine, undertaking), undertaking),
                    None => tools::desk(undertaking),
                };
                let tools = tools::with_web(tools, &web);
                let tools = match &servers {
                    Some(servers) => {
                        tools::with_servers(tools, servers, &tools::Loaded::default(), true)
                    }
                    None => tools,
                };
                match &skills {
                    Some(skills) => tools::with_skills(tools, skills, true),
                    None => tools,
                }
            })
        };
        // What she keeps while she works is hers to have again, and so is
        // what she tells herself from it. A helper given part of the work
        // does neither: it reports to the work.
        let tools = equip(&undertaking)
            .with(tools::Keep::new(undertaking.clone()))
            .with(tools::Tell::new(undertaking.clone()));
        // With someone to ask, she can stop and ask.
        let tools = if next.asked_by.is_some() {
            tools.with(tools::Ask::new(undertaking.clone()))
        } else {
            tools
        };
        let pause = Arc::new(AtomicBool::new(false));
        let limits = Limits {
            max_steps: self.cfg.work.sitting_steps,
            max_seconds: u64::from(self.cfg.work.sitting_minutes) * 60,
            context_tokens: self.cfg.work.context_tokens,
            parts: usize::try_from(self.cfg.work.parts).unwrap_or(usize::MAX),
            part_steps: self.cfg.work.part_steps,
            ..Limits::default()
        };
        // Not who she is: the work is done for her, not as her, and what
        // comes of it reaches people only in her words. What working has
        // taught her goes with it. The same words at every sitting, so that
        // a sitting picked up later reads its transcript from the
        // provider's cache instead of afresh.
        let who = format!(
            "{}{}",
            self.prompts.working.trim_end(),
            self.practice_block()?
        );
        info!("sitting down to {} ({})", next.id, clip(&next.brief, 80));
        self.working.insert(
            next.id.clone(),
            Working {
                pause: pause.clone(),
                record: undertaking.clone(),
                stopped_by: None,
            },
        );
        self.work_at(now + GLANCE);
        let id = next.id.clone();
        let worker = Worker {
            brain,
            tools,
            undertaking,
            limits,
            who,
            machine: place,
            pause,
            helpers: Some(equip),
            prompts: shared,
            agents: self.workshop.agents.clone(),
        };
        self.jobs.spawn(async move {
            // Its folder is made before its first step, out of her way.
            if let Some(dir) = folder {
                let path = std::path::PathBuf::from(&dir);
                match tokio::task::spawn_blocking(move || ghost_work::folder::prepare(&path)).await
                {
                    Ok(Ok(true)) => info!("made {dir} for the work"),
                    Ok(Ok(false)) => {}
                    Ok(Err(e)) => warn!("could not make {dir}: {e}"),
                    Err(e) => warn!("could not make {dir}: {e}"),
                }
            }
            let report = worker.run().await;
            Outcome::Worked {
                id,
                before,
                report,
                progress: worker
                    .undertaking
                    .state()
                    .progress(&ghost_work::prompts::hold(&worker.prompts)),
                record: worker.undertaking,
            }
        });
        Ok(())
    }

    /// What she kept while working on `id` becomes hers, kept as anything
    /// else she keeps is. The record of the work lists everything kept in
    /// it, in every sitting; what she has already kept from it with the
    /// same note, even if she has since let it go, is not kept twice.
    /// Returns the notes of what was kept just now.
    fn take_home(&mut self, id: &str, kept: &[Kept]) -> Result<Vec<String>> {
        let mut new = Vec::new();
        for k in kept {
            if self.ghost.memory().ever_kept(&k.file, &k.note, id)? {
                continue;
            }
            info!("kept {} from {id}", k.file);
            self.record(Event::Kept {
                file: k.file.clone(),
                note: k.note.clone(),
                from: Some(id.to_owned()),
            })?;
            new.push(k.note.clone());
        }
        Ok(new)
    }

    /// What the work `id` has told her and kept that she has not had yet,
    /// from its record, into her journal. Her journal is the receipt: what
    /// it already holds is not written twice, however often she looks and
    /// whichever process the work ran in.
    ///
    /// Most of it waits until she next looks at anything. Something she
    /// could hand over now, a kept image or a file made, also brings her
    /// back to whoever asked for the work, as something she means to do.
    fn hear_from_work(&mut self, id: &str, state: &State, now: DateTime<Utc>) -> Result<()> {
        let heard = self.ghost.memory().reported(id)?;
        let mut to_hand_over = Vec::new();
        let mut made = false;
        for told in state.told.iter().skip(heard) {
            info!("{id} says: {}", clip(&told.text, 120));
            self.record(Event::Reported {
                id: id.to_owned(),
                text: told.text.clone(),
                files: told.files.clone(),
            })?;
            if !told.files.is_empty() {
                made = true;
                to_hand_over.push(format!(
                    "{} ({})",
                    clip(&told.text, 200),
                    told.files.join(", ")
                ));
            }
        }
        to_hand_over.extend(
            self.take_home(id, &state.kept)?
                .iter()
                .map(|note| clip(note, 200)),
        );
        if to_hand_over.is_empty() {
            return Ok(());
        }
        let Some(u) = self.ghost.memory().undertaking(id)? else {
            return Ok(());
        };
        if u.asked_by.is_none() && u.place.is_none() {
            return Ok(());
        }
        // A kept image is read from her own directory; a file made is read
        // off her machine, so it can only be sent where she has one.
        let can_send = (!made || self.workshop.machine.is_some())
            && u.place
                .as_deref()
                .and_then(ghost_core::social::split_key)
                .is_some_and(|(channel, _)| self.can(channel, crate::Can::Files));
        let share = self.prompts.fill(
            "share_found",
            &[
                ("brief", clip(&u.brief, 160)),
                ("found", to_hand_over.join("; ")),
                (
                    "send",
                    self.prompts
                        .text(if can_send { "made_send" } else { "made_tell" })
                        .to_owned(),
                ),
            ],
        );
        if self.intend_as(
            now + Duration::minutes(1),
            share,
            u.asked_by,
            u.place,
            true,
            u.directed,
        )? {
            self.refresh_intentions()?;
        }
        Ok(())
    }

    /// The open piece of work she knows by `number`, or, as a sentence for
    /// her, why there is none.
    pub(super) fn open_work(
        &self,
        number: i64,
    ) -> Result<std::result::Result<ghost_core::memory::Undertaking, String>> {
        let n = [("work", number.to_string())];
        Ok(match self.ghost.memory().undertaking_numbered(number)? {
            Some(u) if u.outcome.is_none() => Ok(u),
            Some(_) => Err(self.prompts.fill("work_over", &n)),
            None => Err(self.prompts.fill("no_work", &n)),
        })
    }

    /// She passes something on to work she has under way. It reaches the
    /// worker before its next step, in this sitting or the next. Returns
    /// why not, if it cannot.
    pub(super) fn steer(
        &mut self,
        number: i64,
        text: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<String>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Some(self.prompts.text("steer_nothing").to_owned()));
        }
        let u = match self.open_work(number)? {
            Ok(u) => u,
            Err(why) => return Ok(Some(why)),
        };
        self.pass_on(u, text, None, now)
    }

    /// Put `text` in the inbox of the open work `u`, from `by` if it was
    /// not she who passed it on, and write it in her journal. Returns why
    /// not, if its record cannot be written to.
    fn pass_on(
        &mut self,
        u: ghost_core::memory::Undertaking,
        text: &str,
        by: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<Option<String>> {
        if let Err(e) = self
            .undertaking(&u.id, &u.brief)
            .and_then(|record| record.add(text))
        {
            warn!("cannot pass anything on to {}: {e}", u.id);
            return Ok(Some(format!("its record could not be written to: {e}")));
        }
        info!("passed on to {}: {}", u.id, clip(text, 80));
        self.record(Event::Added {
            id: u.id,
            text: text.to_owned(),
            by,
        })?;
        self.work_at(now);
        Ok(None)
    }

    /// An operator does `order` to her work from a console, without her:
    /// answers what it asked, or stops it, the way she would. It has to be
    /// work she has open; a part answers to the work it is part of.
    pub(super) fn on_order(
        &mut self,
        order: Order,
        by: &Operator,
        now: DateTime<Utc>,
    ) -> Result<std::result::Result<(), Refused>> {
        let (Order::Answer { work, .. } | Order::Stop { work }) = &order;
        let u = match self.ghost.memory().undertaking(work)? {
            Some(u) if u.outcome.is_none() => u,
            Some(_) => {
                return Ok(Err(Refused::NoSuchWork(format!(
                    "{work} has already ended"
                ))));
            }
            None => return Ok(Err(Refused::NoSuchWork(format!("she has no work {work}")))),
        };
        info!(
            "{} {} from a console",
            by.key,
            match order {
                Order::Answer { .. } => "answers",
                Order::Stop { .. } => "stops",
            }
        );
        let failed = match order {
            Order::Answer { text, .. } => {
                self.pass_on(u, text.trim(), Some(by.key.clone()), now)?
            }
            Order::Stop { .. } => {
                self.stop_open(u, Some(by.name.clone()), now)?;
                None
            }
        };
        Ok(failed.map_or(Ok(()), |why| Err(Refused::Failed(why))))
    }

    /// She stops work she has under way. A sitting on it ends after the
    /// step it is on and comes back as dropped; with none running, it ends
    /// now. Either way what it kept and told is hers first. Returns why
    /// not, if it cannot.
    pub(super) fn stop_work(&mut self, number: i64, now: DateTime<Utc>) -> Result<Option<String>> {
        let u = match self.open_work(number)? {
            Ok(u) => u,
            Err(why) => return Ok(Some(why)),
        };
        self.stop_open(u, None, now)?;
        Ok(None)
    }

    /// Stop the open work `u`, as she does, or as the operator named `by`
    /// does from a console.
    pub(super) fn stop_open(
        &mut self,
        u: ghost_core::memory::Undertaking,
        by: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        if let Some(working) = self.working.get_mut(&u.id) {
            if let Err(e) = working.record.stop() {
                warn!("the record of {} could not be ended: {e}", u.id);
            }
            working.pause.store(true, Ordering::Relaxed);
            working.stopped_by = by;
            info!("stopping {}", u.id);
            return Ok(());
        }
        let progress = match self.undertaking(&u.id, &u.brief) {
            Ok(record) => {
                if let Err(e) = record.stop() {
                    warn!("the record of {} could not be ended: {e}", u.id);
                }
                let state = record.state();
                self.hear_from_work(&u.id, &state, now)?;
                state.progress(self.prompts.store())
            }
            Err(e) => {
                warn!("cannot open the undertaking {}: {e}", u.id);
                u.summary.unwrap_or_default()
            }
        };
        info!("stopped {}", u.id);
        self.dropped(&u.id, &progress, by.as_deref())
    }

    /// The work `id` ends because she stopped it, or the operator named
    /// `by` did.
    fn dropped(&mut self, id: &str, progress: &str, by: Option<&str>) -> Result<()> {
        let progress = ("progress", progress.to_owned());
        let summary = match by {
            Some(name) => self
                .prompts
                .fill("stopped_by", &[("name", name.to_owned()), progress]),
            None => self.prompts.fill("stopped", &[progress]),
        };
        self.record(Event::Concluded {
            id: id.to_owned(),
            outcome: "dropped".into(),
            summary,
        })?;
        Ok(())
    }

    /// A sitting ended.
    pub(super) fn on_worked(
        &mut self,
        id: &str,
        before: u64,
        report: &Report,
        progress: String,
        record: &Undertaking,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let stopped_by = self.working.remove(id).and_then(|w| w.stopped_by);
        // Whatever it said and kept up to the end.
        self.hear_from_work(id, &record.state(), now)?;
        let steps = u32::try_from(report.steps.saturating_sub(before)).unwrap_or(u32::MAX);
        let secs = f64::from(self.cfg.work.rest_minutes) * 60.0 * f64::from(self.cfg.tempo);
        #[allow(clippy::cast_possible_truncation)]
        let rest = Duration::seconds(secs as i64);
        // A slot is free: look at what is next now, whatever this work does.
        self.work_at(now);
        let outcome = match &report.ended {
            Ended::Finished => "finished",
            Ended::Blocked => "blocked",
            Ended::GaveUp => "gave_up",
            Ended::Dropped => {
                info!("{id} stopped after {} steps", report.steps);
                self.dropped(id, &progress, stopped_by.as_deref())?;
                return Ok(());
            }
            Ended::Failed(why) => {
                warn!("the sitting on {id} failed: {why}");
                self.record(Event::Worked {
                    id: id.to_owned(),
                    summary: self
                        .prompts
                        .fill("could_not_think", &[("why", clip(why, 200))]),
                    steps,
                })?;
                self.rest(id, now + Duration::minutes(10).max(rest));
                return Ok(());
            }
            Ended::Waiting => {
                let question = report.asking.clone().unwrap_or_default();
                info!("{id} waits on an answer: {}", clip(&question, 120));
                self.record(Event::Worked {
                    id: id.to_owned(),
                    summary: progress,
                    steps,
                })?;
                self.record(Event::Asked {
                    id: id.to_owned(),
                    question: question.clone(),
                })?;
                if let Some(u) = self.ghost.memory().undertaking(id)? {
                    let options = record.state().waiting_options;
                    let choices = if options.is_empty() {
                        String::new()
                    } else {
                        self.prompts
                            .fill("ask_options", &[("options", options.join(" / "))])
                    };
                    let ask = self.prompts.fill(
                        "ask_them",
                        &[
                            ("brief", clip(&u.brief, 160)),
                            ("question", format!("{}{choices}", clip(&question, 600))),
                        ],
                    );
                    self.intend_as(
                        now + Duration::minutes(1),
                        ask,
                        u.asked_by,
                        u.place,
                        true,
                        u.directed,
                    )?;
                    self.refresh_intentions()?;
                }
                // Something else may be waiting to be done meanwhile.
                self.rest(id, now + rest);
                return Ok(());
            }
            // Going nowhere: it kept talking without doing anything, or a
            // command-line agent ended without saying the work was done.
            // Another sitting from the same place would go the same way, so
            // it comes back to her unfinished.
            Ended::Stalled => {
                info!("{id} stalled after {} steps", report.steps);
                let summary = self.prompts.fill("stalled", &[("progress", progress)]);
                if let Err(e) = record.end(undertaking::Outcome::Blocked, &summary) {
                    warn!("the record of {id} could not be ended: {e}");
                }
                return self.end_work(id, "blocked", &summary, None, &report.files, now);
            }
            Ended::OutOfSteps | Ended::OutOfTime | Ended::Paused => {
                self.record(Event::Worked {
                    id: id.to_owned(),
                    summary: progress,
                    steps,
                })?;
                // A sitting that only ran up against its limits is followed
                // straight on. One that was paused rests first.
                let straight_on = matches!(report.ended, Ended::OutOfSteps | Ended::OutOfTime);
                if !straight_on {
                    self.rest(id, now + rest);
                }
                return Ok(());
            }
        };
        info!("{id} {outcome} after {} steps", report.steps);
        let summary = if report.summary.trim().is_empty() {
            progress
        } else {
            report.summary.trim().to_owned()
        };
        self.end_work(
            id,
            outcome,
            &summary,
            report.lesson.as_deref(),
            &report.files,
            now,
        )
    }

    /// Work her journal has open whose record has ended, and that no
    /// sitting here is on: a sitting, or a stop, whose end she did not
    /// write down before she stopped. It ends in her journal now, as its
    /// record says, with whatever it told her and kept first. Returns
    /// whether any did.
    fn settle_ended(
        &mut self,
        open: &[ghost_core::memory::Undertaking],
        now: DateTime<Utc>,
    ) -> Result<bool> {
        let idle: Vec<String> = open
            .iter()
            .filter(|u| !self.working.contains_key(&u.id))
            .map(|u| u.id.clone())
            .collect();
        if idle.is_empty() {
            return Ok(false);
        }
        let ended: Vec<Look> = self
            .records_of(&idle)
            .into_iter()
            .filter(|look| look.state.finish.is_some())
            .collect();
        for look in &ended {
            let Some(finish) = &look.state.finish else {
                continue;
            };
            let id = &look.id;
            info!("{id} had ended on its record: {}", finish.outcome.word());
            self.hear_from_work(id, &look.state, now)?;
            let progress = look.state.progress(self.prompts.store());
            match finish.outcome {
                undertaking::Outcome::Dropped => self.dropped(id, &progress, None)?,
                outcome => {
                    let summary = match finish.summary.trim() {
                        "" => progress,
                        said => said.to_owned(),
                    };
                    self.end_work(
                        id,
                        outcome.word(),
                        &summary,
                        finish.lesson.as_deref(),
                        &finish.files,
                        now,
                    )?;
                }
            }
        }
        Ok(!ended.is_empty())
    }

    /// The work `id` ends with `outcome`. What it taught her about working
    /// is kept, and whoever asked for it is to be told, finished or not:
    /// what to do about work that did not get done is hers to decide with
    /// them.
    fn end_work(
        &mut self,
        id: &str,
        outcome: &str,
        summary: &str,
        lesson: Option<&str>,
        files: &[String],
        now: DateTime<Utc>,
    ) -> Result<()> {
        let undertaking = self.ghost.memory().undertaking(id)?;
        self.record(Event::Concluded {
            id: id.to_owned(),
            outcome: outcome.to_owned(),
            summary: summary.to_owned(),
        })?;
        if let Some(lesson) = lesson {
            self.on_practice(lesson)?;
        }
        // What a review of her own writing came to becomes part of her craft.
        if id.starts_with(super::public::REVIEW) && outcome == "finished" {
            self.learn(super::public::conclusions(&self.prompts, summary))?;
        }
        // If someone asked, she means to tell them. How and whether is
        // decided when the moment comes, like any other intention.
        if let Some(u) = undertaking
            && (u.asked_by.is_some() || u.place.is_some())
        {
            let how = self.prompts.text(match outcome {
                "finished" => "how_finished",
                "blocked" => "how_blocked",
                _ => "how_gave_up",
            });
            // What she made, if it is files, and whether this place can
            // take a file at all.
            let made = if files.is_empty() {
                String::new()
            } else {
                let can_send = self.workshop.machine.is_some()
                    && u.place
                        .as_deref()
                        .and_then(ghost_core::social::split_key)
                        .is_some_and(|(channel, _)| self.can(channel, crate::Can::Files));
                self.prompts.fill(
                    "made",
                    &[
                        ("files", files.join(", ")),
                        (
                            "send",
                            self.prompts
                                .text(if can_send { "made_send" } else { "made_tell" })
                                .to_owned(),
                        ),
                    ],
                )
            };
            let mut tell = self.prompts.fill(
                "tell_them",
                &[
                    ("brief", clip(&u.brief, 160)),
                    ("how", how.to_owned()),
                    ("summary", clip(summary, 6000)),
                    ("made", made),
                ],
            );
            if outcome != "finished" {
                tell.push_str(self.prompts.text("unfinished_next"));
            }
            // Whoever waits at a terminal hears as soon as it is done;
            // elsewhere the news keeps a minute, the way a person comes
            // back to a chat rather than answering on the instant.
            let waited_on = u
                .place
                .as_deref()
                .and_then(ghost_core::social::split_key)
                .is_some_and(|(channel, _)| self.facts(channel).waited_on);
            let due = if waited_on {
                now
            } else {
                now + Duration::minutes(1)
            };
            self.intend_as(due, tell, u.asked_by, u.place, true, u.directed)?;
            self.refresh_intentions()?;
        }
        Ok(())
    }

    /// The work `id` is not sat down to again before `until`.
    fn rest(&mut self, id: &str, until: DateTime<Utc>) {
        self.resting.insert(id.to_owned(), until);
        self.work_at(until);
    }
}

/// A folder's name from a brief: its first line, in at most 24 letters or
/// figures of any script, the rest made single dashes; `work` if nothing
/// is left.
pub(super) fn folder_name(brief: &str) -> String {
    let line = brief
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let mut name = String::new();
    let mut letters = 0;
    for c in line.chars() {
        if letters == 24 {
            break;
        }
        if c.is_alphanumeric() {
            name.extend(c.to_lowercase());
            letters += 1;
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    let name = name.trim_end_matches('-');
    if name.is_empty() {
        "work".to_owned()
    } else {
        name.to_owned()
    }
}

/// Her machine as a piece of work has it: in the work's own folder, when
/// it has one the machine can be given.
fn in_folder(
    machine: &Arc<dyn ghost_work::Machine>,
    undertaking: &Undertaking,
) -> Arc<dyn ghost_work::Machine> {
    undertaking
        .state()
        .dir
        .and_then(|dir| machine.at(std::path::Path::new(&dir)))
        .unwrap_or_else(|| machine.clone())
}
