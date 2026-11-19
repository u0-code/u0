//! The brains a running daemon holds, and the `brains.*` methods of the
//! Ghost Protocol that read and change them.
//!
//! The brains are the daemon's, shared by every character it runs, and
//! only a daemon that runs a home changes them: its `ghostd.toml` is its
//! own, and its keys are kept in its `.env`. A daemon given a
//! configuration by `--config` lists its brains and leaves the file to
//! whoever wrote it.
//!
//! A change to the brain she runs on takes effect by her mind being
//! started again, at once, by the keeper that starts it again after a
//! failure: stopped in order, made anew with the brains the daemon has
//! now, her presences connected throughout and what they hear in between
//! held for the new mind. It is not swapped under a mind that is running.
//! A mind is made with its brains and keeps them: what it has under way
//! holds the brain it began with, her work is handed the same ones, and
//! her journal's `brain_changed` is written as a mind starts. Starting it
//! again keeps all of that true for the price of what was under way at
//! that moment, which an operator changing her brain is choosing anyway.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};

use ghost_brain::{
    cli_models,
    config::ProviderConfig,
    presets::{PRESETS, Preset},
};
use ghost_protocol::{
    BrainAsk, BrainInfo, BrainList, BrainModel, BrainModels, BrainPreset, BrainRemove, BrainSave,
    BrainUse, Frame, code, method,
};
use serde_json::Value as Json;
use toml_edit::DocumentMut;
use tracing::info;

use super::file;
use crate::{
    cast::Cast,
    config::{self, Config},
    home,
};

/// How long an API has to list its models.
const LIST_WITHIN: Duration = Duration::from_secs(20);
/// How long a brain has to think once when it is checked: a command-line
/// agent starts slowly, and its first request is its slowest.
const THINK_WITHIN: Duration = Duration::from_secs(90);
/// The Anthropic API version the brain itself speaks.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Why a request was refused: its error code, and what to say.
type Refusal = (i64, String);

/// The brains a running daemon holds: the configuration its minds are made
/// with now, and where it keeps them, if it keeps them itself.
pub(crate) struct Held {
    /// The home the daemon runs, whose configuration and keys it changes.
    home: Option<PathBuf>,
    /// The configuration file it was started with, home or not: where a
    /// brain's table is read to lay a draft over it.
    file: Option<PathBuf>,
    now: RwLock<Arc<Config>>,
    /// One change at a time, so that none is half seen by another.
    changing: tokio::sync::Mutex<()>,
}

impl Held {
    /// The brains of `cfg`, read from `file` if there is one, kept in
    /// `home` if the daemon runs one.
    pub(crate) fn new(cfg: Arc<Config>, file: Option<PathBuf>, home: Option<PathBuf>) -> Self {
        Self {
            home,
            file,
            now: RwLock::new(cfg),
            changing: tokio::sync::Mutex::new(()),
        }
    }

    /// The configuration a mind is made with now.
    pub(crate) fn now(&self) -> Arc<Config> {
        self.now
            .read()
            .map_or_else(|e| e.into_inner().clone(), |c| c.clone())
    }

    fn swap(&self, cfg: Config) {
        let cfg = Arc::new(cfg);
        match self.now.write() {
            Ok(mut now) => *now = cfg,
            Err(e) => *e.into_inner() = cfg,
        }
    }

    /// What `brains.list` answers.
    pub(crate) async fn list(&self) -> BrainList {
        let cfg = self.now();
        let mut brains = Vec::with_capacity(cfg.providers.len());
        for (name, p) in &cfg.providers {
            brains.push(info(name, p, &cfg).await);
        }
        BrainList {
            brains,
            think: Some(cfg.brain.think.clone()),
            tick: Some(cfg.brain.tick.clone()),
            changeable: self.home.is_some(),
            presets: PRESETS.iter().map(preset).collect(),
        }
    }

    /// The home, or why the brains cannot be changed without one.
    fn home(&self) -> Result<&Path, Refusal> {
        self.home.as_deref().ok_or_else(|| {
            (
                code::UNCHANGEABLE,
                "this daemon's brains are not changed from a console: it was given its \
                 configuration with --config, which is whoever wrote it's to change; run it \
                 with --home for a core that keeps its own"
                    .to_owned(),
            )
        })
    }

    /// `brains.save`: write the brain, and its key if one is given, and
    /// start every mind again if it is the one they run on.
    pub(crate) async fn save(&self, save: BrainSave, cast: &Cast) -> Result<BrainList, Refusal> {
        let home = self.home()?;
        let _one = self.changing.lock().await;
        let (path, mut doc) = read(home)?;
        let name = save.name.trim().to_owned();
        if let Some(was) = save.was.as_deref()
            && !saved(&doc, was)
        {
            return Err(no_such(was));
        }
        let set = save
            .set
            .iter()
            .map(|(k, v)| file::value(v).map(|v| (k.clone(), v)))
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(invalid)?;
        file::write(&mut doc, save.was.as_deref(), &name, &set, &save.clear).map_err(invalid)?;
        let text = doc.to_string();
        let mut cfg = runs(&text)?;
        let mut keys = self.now().keys.clone();
        let env = home.join(home::ENV);
        let mut env_text = None;
        if let Some(key) = save.key.as_deref() {
            let var = key_var(&name, cfg.providers.get(&name))?;
            let was = std::fs::read_to_string(&env).unwrap_or_default();
            env_text = Some(file::with_key(&was, &var, key).map_err(invalid)?);
            keys.insert(var, key.trim().to_owned());
        }
        if let Some(env_text) = env_text {
            file::write_env(&env, &env_text).map_err(internal)?;
        }
        home::write_whole(&path, &text).map_err(internal)?;
        cfg.keys = keys;
        let in_use = file::in_use(&doc, &name);
        self.swap(cfg);
        info!(
            "an operator saved the brain {name}{}",
            if save.key.is_some() {
                ", with its key"
            } else {
                ""
            }
        );
        if in_use {
            cast.start_again();
        }
        Ok(self.list().await)
    }

    /// `brains.remove`: take out a brain nobody runs on.
    pub(crate) async fn remove(&self, gone: BrainRemove) -> Result<BrainList, Refusal> {
        let home = self.home()?;
        let _one = self.changing.lock().await;
        let (path, mut doc) = read(home)?;
        if !saved(&doc, &gone.name) {
            return Err(no_such(&gone.name));
        }
        file::remove(&mut doc, &gone.name).map_err(invalid)?;
        let text = doc.to_string();
        let mut cfg = runs(&text)?;
        cfg.keys = self.now().keys.clone();
        home::write_whole(&path, &text).map_err(internal)?;
        self.swap(cfg);
        info!("an operator took out the brain {}", gone.name);
        Ok(self.list().await)
    }

    /// `brains.use`: once the brain has shown it can think, have her think
    /// and tick with it, and start every mind again on it.
    pub(crate) async fn choose(&self, chosen: BrainUse, cast: &Cast) -> Result<BrainList, Refusal> {
        let home = self.home()?;
        let cfg = self.now();
        let p = cfg
            .providers
            .get(&chosen.name)
            .ok_or_else(|| no_such(&chosen.name))?;
        let changes_model = chosen.model.as_ref().is_some_and(|m| *m != model_of(p));
        if cfg.brain.think == chosen.name && cfg.brain.tick == chosen.name && !changes_model {
            return Ok(self.list().await);
        }
        let mut candidate = cfg.keyed(p);
        if let Some(model) = &chosen.model {
            set_model(&mut candidate, model);
        }
        think_once(&candidate).await?;

        let _one = self.changing.lock().await;
        let (path, mut doc) = read(home)?;
        if !saved(&doc, &chosen.name) {
            return Err(no_such(&chosen.name));
        }
        file::choose(&mut doc, &chosen.name, chosen.model.as_deref()).map_err(invalid)?;
        let text = doc.to_string();
        let mut new = runs(&text)?;
        new.keys = self.now().keys.clone();
        home::write_whole(&path, &text).map_err(internal)?;
        self.swap(new);
        info!(
            "an operator has her think with {}{}",
            chosen.name,
            chosen
                .model
                .as_deref()
                .map(|m| format!(" on {}", if m.is_empty() { "its default" } else { m }))
                .unwrap_or_default()
        );
        cast.start_again();
        Ok(self.list().await)
    }

    /// `brains.models`: what the brain asked about offers, found on this
    /// machine: what its API lists, or what the agent installed here knows.
    pub(crate) async fn models(&self, ask: &BrainAsk) -> Result<BrainModels, Refusal> {
        let p = self.provider(ask)?;
        let models = match p.agent() {
            Some(agent) => tokio::task::spawn_blocking(move || cli_models::offered(&agent))
                .await
                .map_err(|e| internal(e.into()))?
                .map_err(|why| (code::BRAIN_FAILED, why))?
                .into_iter()
                .map(|o| BrainModel {
                    id: o.id,
                    name: o.name,
                })
                .collect(),
            None => list_models(&p)
                .await?
                .into_iter()
                .map(|id| BrainModel {
                    id,
                    name: String::new(),
                })
                .collect(),
        };
        Ok(BrainModels { models })
    }

    /// `brains.check`: the brain asked about thinks once.
    pub(crate) async fn check(&self, ask: &BrainAsk) -> Result<Json, Refusal> {
        let p = self.provider(ask)?;
        think_once(&p).await?;
        Ok(serde_json::json!({}))
    }

    /// The brain `ask` names or describes, with the key the daemon holds
    /// for it and the model asked for.
    fn provider(&self, ask: &BrainAsk) -> Result<ProviderConfig, Refusal> {
        let cfg = self.now();
        let mut table = toml::Table::new();
        if let Some(name) = &ask.name {
            let saved = cfg.providers.get(name).ok_or_else(|| no_such(name))?;
            if ask.provider.is_none() {
                let mut p = cfg.keyed(saved);
                if let Some(model) = &ask.model {
                    set_model(&mut p, model);
                }
                return Ok(p);
            }
            if let Some(found) = self.table(name) {
                table = found;
            }
        }
        if let Some(draft) = &ask.provider {
            let over: toml::Table = serde_json::from_value(Json::Object(draft.clone()))
                .map_err(|e| invalid(anyhow::anyhow!("`provider`: {e}")))?;
            config::lay_over(&mut table, &over);
        }
        if table.is_empty() {
            return Err((
                code::INVALID_PARAMS,
                "name a brain with `name`, or describe one with `provider`".to_owned(),
            ));
        }
        let p: ProviderConfig = toml::Value::Table(table)
            .try_into()
            .map_err(|e| invalid(anyhow::anyhow!("{e}")))?;
        let mut p = cfg.keyed(&p);
        if let Some(model) = &ask.model {
            set_model(&mut p, model);
        }
        Ok(p)
    }

    /// The table of the brain `name` as the configuration file has it.
    fn table(&self, name: &str) -> Option<toml::Table> {
        let text = std::fs::read_to_string(self.file.as_ref()?).ok()?;
        let all: toml::Table = toml::from_str(&text).ok()?;
        all.get("providers")?.get(name)?.as_table().cloned()
    }
}

/// Answer the `brains.*` request `frame`, id `id`, from a console that
/// `owner` says is the daemon's owner's. Anyone may list them; only an
/// operator changes them or asks them anything, which spends their keys.
pub(crate) async fn answer(id: u64, frame: &Frame, owner: bool, held: &Held, cast: &Cast) -> Frame {
    let asked = frame.method.as_deref().unwrap_or_default();
    if asked == method::BRAINS {
        return Frame::ok(id, &held.list().await);
    }
    if !owner {
        return Frame::err(
            id,
            code::UNAUTHORIZED,
            "only the daemon's owner changes or asks its brains from a console: give the \
             daemon's token",
        );
    }
    let done = match asked {
        method::BRAIN_SAVE => match frame.params::<BrainSave>() {
            Ok(save) => held.save(save, cast).await.map(to_json),
            Err(e) => Err((e.code, e.message)),
        },
        method::BRAIN_REMOVE => match frame.params::<BrainRemove>() {
            Ok(gone) => held.remove(gone).await.map(to_json),
            Err(e) => Err((e.code, e.message)),
        },
        method::BRAIN_USE => match frame.params::<BrainUse>() {
            Ok(chosen) => held.choose(chosen, cast).await.map(to_json),
            Err(e) => Err((e.code, e.message)),
        },
        method::BRAIN_MODELS => match frame.params::<BrainAsk>() {
            Ok(ask) => held.models(&ask).await.map(to_json),
            Err(e) => Err((e.code, e.message)),
        },
        method::BRAIN_CHECK => match frame.params::<BrainAsk>() {
            Ok(ask) => held.check(&ask).await,
            Err(e) => Err((e.code, e.message)),
        },
        other => Err((code::METHOD_NOT_FOUND, format!("unknown method {other:?}"))),
    };
    match done {
        Ok(value) => Frame::ok(id, &value),
        Err((code, why)) => Frame::err(id, code, why),
    }
}

fn to_json(value: impl serde::Serialize) -> Json {
    serde_json::to_value(value).unwrap_or(Json::Null)
}

/// The home's configuration, as its path and its text to edit.
fn read(home: &Path) -> Result<(PathBuf, DocumentMut), Refusal> {
    let path = home.join(home::CONFIG);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| internal(anyhow::anyhow!("read {}: {e}", path.display())))?;
    let doc = text
        .parse::<DocumentMut>()
        .map_err(|e| internal(anyhow::anyhow!("parse {}: {e}", path.display())))?;
    Ok((path, doc))
}

/// The configuration `text` would run with, or why it would not.
fn runs(text: &str) -> Result<Config, Refusal> {
    let cfg = Config::parse(text).map_err(|e| {
        (
            code::INVALID_PARAMS,
            format!("the configuration would not run with that: {e:#}"),
        )
    })?;
    cfg.tiers_named().map_err(invalid)?;
    Ok(cfg)
}

/// Whether `[providers.<name>]` is there.
fn saved(doc: &DocumentMut, name: &str) -> bool {
    doc.get("providers")
        .and_then(|p| p.get(name))
        .is_some_and(toml_edit::Item::is_table_like)
}

fn no_such(name: &str) -> Refusal {
    (code::NO_SUCH_BRAIN, format!("no brain is saved as {name}"))
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "it is what map_err is handed"
)]
fn invalid(e: anyhow::Error) -> Refusal {
    (code::INVALID_PARAMS, format!("{e:#}"))
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "it is what map_err is handed"
)]
fn internal(e: anyhow::Error) -> Refusal {
    (code::INTERNAL, format!("{e:#}"))
}

/// The variable the brain `name`, configured as `p`, reads its key from,
/// for a key given with it; or why it takes none.
fn key_var(name: &str, p: Option<&ProviderConfig>) -> Result<String, Refusal> {
    match p {
        Some(
            ProviderConfig::Anthropic {
                api_key: None,
                api_key_env,
                ..
            }
            | ProviderConfig::Openai {
                api_key: None,
                api_key_env,
                ..
            },
        ) => Ok(api_key_env.clone()),
        Some(ProviderConfig::Anthropic { .. } | ProviderConfig::Openai { .. }) => Err((
            code::INVALID_PARAMS,
            format!("{name} is given a key in the configuration; take that out first"),
        )),
        _ => Err((
            code::INVALID_PARAMS,
            format!(
                "{name} is a command-line agent, which is signed in to rather than given a key"
            ),
        )),
    }
}

/// The model `p` is set to; empty for an agent's own default.
fn model_of(p: &ProviderConfig) -> String {
    match p {
        ProviderConfig::Anthropic { model, .. } | ProviderConfig::Openai { model, .. } => {
            model.clone()
        }
        ProviderConfig::ClaudeCode { model, .. } | ProviderConfig::Codex { model, .. } => {
            model.clone().unwrap_or_default()
        }
    }
}

/// Have `p` think with `model`; an empty one is an agent's own default.
fn set_model(p: &mut ProviderConfig, to: &str) {
    match p {
        ProviderConfig::Anthropic { model, .. } | ProviderConfig::Openai { model, .. } => {
            to.clone_into(model);
        }
        ProviderConfig::ClaudeCode { model, .. } | ProviderConfig::Codex { model, .. } => {
            *model = (!to.is_empty()).then(|| to.to_owned());
        }
    }
}

/// Whether the environment variable `var` holds something.
fn env_set(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| !v.trim().is_empty())
}

/// A preset as `brains.list` offers it.
fn preset(p: &Preset) -> BrainPreset {
    let provider = match serde_json::to_value(p.table()) {
        Ok(Json::Object(table)) => table,
        _ => serde_json::Map::new(),
    };
    BrainPreset {
        id: p.id.to_owned(),
        name: p.name.to_owned(),
        provider,
    }
}

/// One brain as `brains.list` has it, with whether this machine can run it.
async fn info(name: &str, p: &ProviderConfig, cfg: &Config) -> BrainInfo {
    let mut out = BrainInfo {
        name: name.to_owned(),
        kind: p.kind().to_owned(),
        model: model_of(p),
        ..BrainInfo::default()
    };
    match p {
        ProviderConfig::Anthropic {
            api_key,
            api_key_env,
            base_url,
            ..
        }
        | ProviderConfig::Openai {
            api_key,
            api_key_env,
            base_url,
            ..
        } => {
            out.base_url.clone_from(base_url);
            out.preset = Preset::of(&out.kind, base_url.as_deref()).map(|p| p.id.to_owned());
            if api_key.is_some() {
                out.key_set = true;
            } else {
                out.key_env = Some(api_key_env.clone());
                out.key_set = cfg.keys.contains_key(api_key_env) || env_set(api_key_env);
            }
            let base = base_url.as_deref().unwrap_or_default();
            out.blocker = if out.key_set {
                let (authority, _) = file::authority(base);
                (file::on_this_machine(base) && !answers(&authority).await)
                    .then(|| format!("nothing answers at {authority}"))
            } else {
                Some(format!("{api_key_env} is not set"))
            };
        }
        ProviderConfig::ClaudeCode { program, .. } | ProviderConfig::Codex { program, .. } => {
            let program = program.clone().unwrap_or_else(|| {
                if out.kind == "codex" {
                    "codex"
                } else {
                    "claude"
                }
                .to_owned()
            });
            out.blocker = cli_models::find(&program)
                .is_none()
                .then(|| format!("`{program}` was not found on this machine"));
            out.program = Some(program);
            out.preset = Preset::of(&out.kind, None).map(|p| p.id.to_owned());
        }
    }
    out
}

/// Whether something listens at `host:port`, asked quickly.
async fn answers(authority: &str) -> bool {
    let connect = tokio::net::TcpStream::connect(authority);
    matches!(
        tokio::time::timeout(Duration::from_millis(250), connect).await,
        Ok(Ok(_))
    )
}

/// Ask `p` to think once, as little as there is, the way her mind would.
async fn think_once(p: &ProviderConfig) -> Result<(), Refusal> {
    let brain = p.build().map_err(|e| match e {
        ghost_brain::Error::MissingKey(var) => (code::NO_KEY, format!("{var} is not set")),
        e => (code::BRAIN_FAILED, e.to_string()),
    })?;
    let ask = super::hello();
    match tokio::time::timeout(THINK_WITHIN, brain.complete(&ask)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err((code::BRAIN_FAILED, e.to_string())),
        Err(_) => Err((
            code::BRAIN_FAILED,
            format!("it did not answer in {}s", THINK_WITHIN.as_secs()),
        )),
    }
}

/// The key of an API brain: given, or in the environment.
fn key_of(api_key: Option<&String>, var: &str) -> Result<String, Refusal> {
    api_key
        .cloned()
        .or_else(|| std::env::var(var).ok())
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| (code::NO_KEY, format!("{var} is not set")))
}

/// The models `p`'s API lists, asked through the one network setting.
async fn list_models(p: &ProviderConfig) -> Result<Vec<String>, Refusal> {
    let failed = |why: String| (code::BRAIN_FAILED, why);
    let client = ghost_net::builder()
        .timeout(LIST_WITHIN)
        .build()
        .map_err(|e| failed(sent_error(&e)))?;
    let (request, sorted) = match p {
        ProviderConfig::Anthropic {
            api_key,
            api_key_env,
            base_url,
            ..
        } => {
            let key = key_of(api_key.as_ref(), api_key_env)?;
            let base = base_url
                .as_deref()
                .unwrap_or(ghost_brain::anthropic::DEFAULT_BASE_URL)
                .trim_end_matches('/');
            (
                client
                    .get(format!("{base}/v1/models?limit=1000"))
                    .header("x-api-key", key)
                    .header("anthropic-version", ANTHROPIC_VERSION),
                // Anthropic lists its newest first, which is the order to keep.
                false,
            )
        }
        ProviderConfig::Openai {
            api_key,
            api_key_env,
            base_url,
            ..
        } => {
            let key = key_of(api_key.as_ref(), api_key_env)?;
            let base = base_url
                .as_deref()
                .unwrap_or(ghost_brain::openai::DEFAULT_BASE_URL)
                .trim_end_matches('/');
            (
                client
                    .get(format!("{base}/models"))
                    .header("authorization", format!("Bearer {key}")),
                true,
            )
        }
        // An agent's are read where it is installed: see `Held::models`.
        ProviderConfig::ClaudeCode { .. } | ProviderConfig::Codex { .. } => {
            return Err(failed("a command-line agent has no API to list".to_owned()));
        }
    };
    let response = request.send().await.map_err(|e| failed(sent_error(&e)))?;
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    match status {
        200..=299 => {
            let mut ids = parse_models(&body).map_err(failed)?;
            if sorted {
                ids.sort();
            }
            Ok(ids)
        }
        404 | 405 => Err(failed("the server lists no models".to_owned())),
        _ => Err(failed(error_line(status, &body))),
    }
}

/// The models a `models` listing names, by id, from its JSON body: the
/// `data` array both protocols answer with. Gemini's ids come as
/// `models/…`, which its chat endpoint takes without the prefix.
fn parse_models(body: &str) -> Result<Vec<String>, String> {
    let json: Json = serde_json::from_str(body).map_err(|_| first_line(body))?;
    let data = json
        .get("data")
        .and_then(Json::as_array)
        .ok_or_else(|| "no model list in the answer".to_owned())?;
    let mut ids: Vec<String> = data
        .iter()
        .filter_map(|m| m.get("id").and_then(Json::as_str))
        .map(|id| id.strip_prefix("models/").unwrap_or(id).to_owned())
        .collect();
    ids.dedup();
    Ok(ids)
}

/// What went wrong, in one short line: the status and the server's own
/// message where it gave one.
fn error_line(status: u16, body: &str) -> String {
    let said = serde_json::from_str::<Json>(body).ok().and_then(|json| {
        let error = json.get("error").unwrap_or(&json);
        error
            .get("message")
            .and_then(Json::as_str)
            .or_else(|| error.as_str())
            .or_else(|| json.get("message").and_then(Json::as_str))
            .or_else(|| json.get("detail").and_then(Json::as_str))
            .map(str::to_owned)
    });
    let said = said.unwrap_or_else(|| first_line(body));
    if said.is_empty() {
        status.to_string()
    } else {
        format!("{status} · {}", first_line(&said))
    }
}

/// The first non-empty line of `text`, cut short.
fn first_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty());
    let line = line.unwrap_or_default();
    if line.chars().count() > 90 {
        format!("{}…", line.chars().take(89).collect::<String>())
    } else {
        line.to_owned()
    }
}

/// The one line a failed request is told by: its innermost cause.
fn sent_error(e: &(dyn std::error::Error + 'static)) -> String {
    let mut cause = e;
    while let Some(next) = cause.source() {
        cause = next;
    }
    first_line(&cause.to_string())
}

#[cfg(test)]
mod tests;
