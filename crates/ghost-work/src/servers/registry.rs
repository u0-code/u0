//! The MCP registry: what it lists, and whether a listing can be
//! installed here as it stands.
//!
//! The registry is a public index anyone can publish to under a namespace
//! they have shown they own: a GitHub account for `io.github.<account>/…`,
//! a domain for the rest. What it vouches for is that name, and nothing
//! about what the package does. So what is taken from it is checked for
//! shape here, before anything is run: a listing that is not the current
//! one, whose repository is not its owner's, whose versions float, or that
//! needs something nobody has given her, is not installed, and she is told
//! why in a sentence she can act on.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use ghost_core::journal::Launch;
use serde::Deserialize;

use super::guard;
use crate::prompts::{Store, TEXTS};

/// How many listings one search asks for.
const PAGE: usize = 30;
/// The key under `_meta` where the registry says what it knows of a
/// listing.
const OFFICIAL: &str = "io.modelcontextprotocol.registry/official";

/// One listing, as the registry gives it.
#[derive(Debug, Clone, Deserialize)]
pub struct Listing {
    pub server: Server,
    #[serde(rename = "_meta", default)]
    meta: BTreeMap<String, serde_json::Value>,
}

/// What the registry knows of a listing, beside what its publisher said.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Official {
    pub status: String,
    pub is_latest: bool,
    pub published_at: Option<DateTime<Utc>>,
}

/// `server.json`: what a publisher says of their server.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Server {
    pub name: String,
    pub description: String,
    pub version: String,
    pub repository: Option<Repository>,
    pub packages: Vec<Package>,
    pub remotes: Vec<Remote>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Repository {
    pub url: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Package {
    pub registry_type: String,
    pub identifier: String,
    pub version: String,
    pub package_arguments: Vec<Argument>,
    pub environment_variables: Vec<Variable>,
    pub transport: Transport,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Transport {
    #[serde(rename = "type")]
    pub kind: String,
}

/// An argument the package is run with.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Argument {
    #[serde(rename = "type")]
    pub kind: String,
    pub name: Option<String>,
    pub value: Option<String>,
    pub default: Option<String>,
    pub is_required: bool,
    pub is_secret: bool,
}

/// A variable the package reads from its environment.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Variable {
    pub name: String,
    pub default: Option<String>,
    pub is_required: bool,
    pub is_secret: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Remote {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub headers: Vec<Variable>,
}

impl Listing {
    /// What the registry itself says of it.
    pub fn official(&self) -> Official {
        self.meta
            .get(OFFICIAL)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    /// Whether it is the one to take: active, and the latest of its name.
    pub fn current(&self) -> bool {
        let official = self.official();
        official.status == "active" && official.is_latest
    }
}

/// Where the registry is, and how it is asked.
pub struct Registry {
    base: String,
    http: reqwest::Client,
}

impl Registry {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
            http: ghost_net::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent(concat!("ghost/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default(),
        }
    }

    /// The current listings whose names have `query` in them. Superseded
    /// versions and listings that are not active are left out: what is
    /// installed is only ever what the registry says is current. Why there
    /// are none, in `words`.
    pub async fn search(&self, words: &Store, query: &str) -> Result<Vec<Listing>, String> {
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            servers: Vec<Listing>,
        }
        let page: Page = self
            .http
            .get(format!("{}/v0.1/servers", self.base))
            .query(&[
                ("search", query),
                ("version", "latest"),
                ("limit", &PAGE.to_string()),
            ])
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| words.fill(TEXTS, "registry_unreached", &[("why", e.to_string())]))?
            .json()
            .await
            .map_err(|e| words.fill(TEXTS, "registry_unreadable", &[("why", e.to_string())]))?;
        Ok(page.servers.into_iter().filter(Listing::current).collect())
    }

    /// The current listing called exactly `name`. Should the registry
    /// ever list two, the one published first is taken: a later one of
    /// the same name is more likely a copy than the original.
    pub async fn named(&self, words: &Store, name: &str) -> Result<Option<Listing>, String> {
        let mut found: Vec<Listing> = self
            .search(words, name)
            .await?
            .into_iter()
            .filter(|l| l.server.name == name)
            .collect();
        found.sort_by_key(|l| l.official().published_at);
        Ok(found.into_iter().next())
    }
}

/// What installing a listing would run, and what is known of it.
#[derive(Debug, Clone)]
pub struct Plan {
    pub name: String,
    pub version: String,
    pub description: String,
    /// The exact package or remote, as `Installed` keeps it.
    pub source: String,
    pub owner: Option<String>,
    pub launch: Launch,
    /// What it reads from its environment that would be given to it,
    /// by name.
    pub given: Vec<String>,
}

/// What a listing needs from whoever runs her body, as she is shown it in
/// `words`: each variable it requires, with the secret ones marked.
pub fn needs(words: &Store, server: &Server) -> Vec<String> {
    let chosen = server
        .packages
        .iter()
        .find(|p| runner(p).is_some())
        .map(|p| p.environment_variables.as_slice())
        .or_else(|| server.remotes.first().map(|r| r.headers.as_slice()))
        .unwrap_or_default();
    chosen
        .iter()
        .filter(|v| v.is_required && (v.is_secret || v.default.is_none()))
        .map(|v| {
            if v.is_secret {
                words.fill(TEXTS, "secret", &[("name", v.name.clone())])
            } else {
                v.name.clone()
            }
        })
        .collect()
}

/// How a package is run here, if it can be: npm packages with `npx`,
/// `PyPI` packages with `uvx`. Container images and bundles are not run.
fn runner(package: &Package) -> Option<&'static str> {
    match package.registry_type.as_str() {
        "npm" => Some("npm"),
        "pypi" => Some("pypi"),
        _ => None,
    }
}

/// What is run for `listing`, if it can be installed here as it stands,
/// or why not, in `words`. `given` says whether a variable of its
/// environment has been given to it by whoever runs her body.
pub fn plan(
    words: &Store,
    listing: &Listing,
    given: &dyn Fn(&str) -> bool,
) -> Result<Plan, String> {
    let server = &listing.server;
    if !listing.current() {
        return Err(words.fill(TEXTS, "not_current", &[("name", server.name.clone())]));
    }
    let owner = owner(words, server)?;
    let mut why = Vec::new();
    for package in &server.packages {
        match from_package(words, package, given) {
            Ok((source, launch, passed)) => {
                return Ok(Plan {
                    name: server.name.clone(),
                    version: server.version.clone(),
                    description: server.description.clone(),
                    source,
                    owner,
                    launch,
                    given: passed,
                });
            }
            Err(e) => why.push(e),
        }
    }
    for remote in &server.remotes {
        match from_remote(words, remote) {
            Ok(launch) => {
                return Ok(Plan {
                    name: server.name.clone(),
                    version: server.version.clone(),
                    description: server.description.clone(),
                    source: remote.url.clone(),
                    owner,
                    launch,
                    given: Vec::new(),
                });
            }
            Err(e) => why.push(e),
        }
    }
    Err(match why.into_iter().next() {
        Some(first) => first,
        None => words.text(TEXTS, "nothing_to_run").to_owned(),
    })
}

/// Who the registry vouches the name belongs to. A name under
/// `io.github.<account>` was proven to be that account's, so its
/// repository has to be that account's too: a listing that points at
/// someone else's code under a name of its own is how a copy passes for
/// the original. Any other namespace is a reversed domain, which is
/// recorded as it is.
fn owner(words: &Store, server: &Server) -> Result<Option<String>, String> {
    let (namespace, _) = server.name.split_once('/').ok_or_else(|| {
        words.fill(
            TEXTS,
            "not_a_registry_name",
            &[("name", server.name.clone())],
        )
    })?;
    if let Some(account) = namespace.strip_prefix("io.github.") {
        let repository = server.repository.as_ref().map_or("", |r| r.url.as_str());
        let theirs = repository
            .strip_prefix("https://github.com/")
            .and_then(|rest| rest.split('/').next())
            .is_some_and(|owner| owner.eq_ignore_ascii_case(account));
        if !theirs {
            let repository = if repository.is_empty() {
                words.text(TEXTS, "no_repository")
            } else {
                repository
            };
            return Err(words.fill(
                TEXTS,
                "not_the_owners",
                &[
                    ("repository", repository.to_owned()),
                    ("account", account.to_owned()),
                ],
            ));
        }
        return Ok(Some(format!("github.com/{account}")));
    }
    let domain: Vec<&str> = namespace.split('.').rev().collect();
    Ok(Some(domain.join(".")))
}

/// How a package runs: the command, with every version pinned, and the
/// variables it may be given.
fn from_package(
    words: &Store,
    package: &Package,
    given: &dyn Fn(&str) -> bool,
) -> Result<(String, Launch, Vec<String>), String> {
    let said = |key: &str, what: &str| words.fill(TEXTS, key, &[("what", what.to_owned())]);
    let Some(kind) = runner(package) else {
        return Err(said("package_not_run", &package.registry_type));
    };
    if package.transport.kind != "stdio" && !package.transport.kind.is_empty() {
        return Err(said("package_not_stdio", &package.transport.kind));
    }
    if !guard::package_name(&package.identifier) {
        return Err(said(
            "package_name_unplain",
            &format!("{:?}", package.identifier),
        ));
    }
    if !guard::exact_version(&package.version) {
        return Err(said(
            "package_version_unpinned",
            &format!("{:?}", package.version),
        ));
    }
    let mut args = Vec::new();
    for argument in &package.package_arguments {
        let value = argument.value.as_ref().or(argument.default.as_ref());
        let usable = value.filter(|v| !v.contains('{'));
        let label = argument
            .name
            .as_deref()
            .unwrap_or_else(|| words.text(TEXTS, "an_argument"));
        match (usable, argument.is_required) {
            (None, true) if argument.is_secret => {
                return Err(said("needs_ungiven", label));
            }
            (None, true) => {
                return Err(said("needs_choosing", label));
            }
            (None, false) => {}
            (Some(value), _) => {
                if argument.kind == "named"
                    && let Some(name) = &argument.name
                {
                    args.push(name.clone());
                }
                args.push(value.clone());
            }
        }
    }
    let mut launch = Launch::default();
    let mut passed = Vec::new();
    for variable in &package.environment_variables {
        if given(&variable.name) {
            launch.pass.push(variable.name.clone());
            passed.push(variable.name.clone());
            continue;
        }
        if !variable.is_required {
            continue;
        }
        match &variable.default {
            Some(default) if !variable.is_secret => {
                launch.set.push(format!("{}={default}", variable.name));
            }
            _ => {
                return Err(said("needs_ungiven", &variable.name));
            }
        }
    }
    let (id, version) = (&package.identifier, &package.version);
    let (source, mut command) = if kind == "npm" {
        (
            format!("npm:{id}@{version}"),
            // Install scripts are where an npm package runs code before
            // anyone has looked at it; none is run.
            [
                "npx",
                "--yes",
                "--ignore-scripts",
                "--prefer-offline",
                "--",
                &format!("{id}@{version}"),
            ]
            .map(str::to_owned)
            .to_vec(),
        )
    } else {
        (
            format!("pypi:{id}=={version}"),
            // Built from wheels only: building from source runs the
            // package's own code at install time.
            [
                "uvx",
                "--no-build",
                "--from",
                &format!("{id}=={version}"),
                id,
            ]
            .map(str::to_owned)
            .to_vec(),
        )
    };
    command.extend(args);
    launch.command = command;
    Ok((source, launch, passed))
}

/// A remote, if it is one that can be reached as it stands.
fn from_remote(words: &Store, remote: &Remote) -> Result<Launch, String> {
    if remote.kind != "streamable-http" {
        return Err(words.fill(
            TEXTS,
            "remote_not_streamable",
            &[("what", remote.kind.clone())],
        ));
    }
    if let Some(header) = remote.headers.iter().find(|h| h.is_required) {
        return Err(words.fill(TEXTS, "needs_header", &[("what", header.name.clone())]));
    }
    guard::plain_remote(words, &remote.url)?;
    Ok(Launch {
        url: Some(remote.url.clone()),
        ..Launch::default()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    pub fn listing(value: serde_json::Value) -> Listing {
        serde_json::from_value(value).unwrap()
    }

    fn current() -> serde_json::Value {
        json!({OFFICIAL: {"status": "active", "isLatest": true, "publishedAt": "2026-01-01T00:00:00Z"}})
    }

    #[test]
    fn a_namespace_whose_repository_belongs_to_someone_else_is_refused() {
        let copy = listing(json!({
            "server": {
                "name": "io.github.mallory/playwright-mcp",
                "version": "0.0.85",
                "repository": {"url": "https://github.com/microsoft/playwright-mcp"},
                "packages": [{"registryType": "npm", "identifier": "@playwright/mcp", "version": "0.0.85", "transport": {"type": "stdio"}}],
            },
            "_meta": current(),
        }));
        let why = plan(&crate::Prompts::shipped(), &copy, &|_| false).unwrap_err();
        assert!(
            why.contains("does not belong to the namespace's owner"),
            "{why}"
        );
    }

    #[test]
    fn a_secret_nobody_has_given_is_refused_and_a_pinned_command_is_planned_otherwise() {
        let server = |required| {
            listing(json!({
                "server": {
                    "name": "io.github.acme/slack",
                    "version": "1.0.0",
                    "repository": {"url": "https://github.com/acme/slack-mcp"},
                    "packages": [{
                        "registryType": "npm", "identifier": "@acme/slack-mcp", "version": "1.0.0",
                        "transport": {"type": "stdio"},
                        "packageArguments": [{"type": "named", "name": "--mode", "value": "read"}],
                        "environmentVariables": [{"name": "SLACK_TOKEN", "isRequired": required, "isSecret": true}],
                    }],
                },
                "_meta": current(),
            }))
        };
        let why = plan(&crate::Prompts::shipped(), &server(true), &|_| false).unwrap_err();
        assert_eq!(why, "it needs SLACK_TOKEN, which nobody has given you.");
        assert_eq!(
            needs(&crate::Prompts::shipped(), &server(true).server),
            ["SLACK_TOKEN (secret)"]
        );

        let planned = plan(&crate::Prompts::shipped(), &server(true), &|name| {
            name == "SLACK_TOKEN"
        })
        .unwrap();
        assert_eq!(
            planned.launch.command,
            [
                "npx",
                "--yes",
                "--ignore-scripts",
                "--prefer-offline",
                "--",
                "@acme/slack-mcp@1.0.0",
                "--mode",
                "read"
            ]
        );
        assert_eq!(planned.launch.pass, ["SLACK_TOKEN"]);
        assert_eq!(planned.owner.as_deref(), Some("github.com/acme"));
    }

    #[test]
    fn floating_versions_containers_and_old_listings_are_not_installed() {
        let with = |package: serde_json::Value, meta: serde_json::Value| {
            listing(json!({
                "server": {"name": "com.example/x", "version": "1.0.0", "packages": [package]},
                "_meta": meta,
            }))
        };
        let npm = |version: &str| json!({"registryType": "npm", "identifier": "x", "version": version, "transport": {"type": "stdio"}});
        assert!(
            plan(
                &crate::Prompts::shipped(),
                &with(npm("^1.0.0"), current()),
                &|_| false
            )
            .is_err()
        );
        assert!(
            plan(
                &crate::Prompts::shipped(),
                &with(npm("latest"), current()),
                &|_| false
            )
            .is_err()
        );
        let oci = json!({"registryType": "oci", "identifier": "x", "version": "1", "transport": {"type": "stdio"}});
        assert!(
            plan(&crate::Prompts::shipped(), &with(oci, current()), &|_| {
                false
            })
            .is_err()
        );
        let old = json!({OFFICIAL: {"status": "active", "isLatest": false}});
        assert!(
            plan(
                &crate::Prompts::shipped(),
                &with(npm("1.0.0"), old),
                &|_| false
            )
            .is_err()
        );
        let planned = plan(
            &crate::Prompts::shipped(),
            &with(npm("1.0.0"), current()),
            &|_| false,
        )
        .unwrap();
        assert_eq!(planned.owner.as_deref(), Some("example.com"));
    }
}
