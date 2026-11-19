//! `search` and `fetch`: the web.
//!
//! Search has three interchangeable sources: DeepSeek's search API (the same
//! key as the model), Brave, or a SearXNG you run yourself. Fetch asks
//! DeepSeek's fetch API for the page as Markdown when that key is present,
//! and otherwise gets the page itself and strips it down to text. Either
//! way the images a page shows are listed after its text, and an address
//! that holds an image comes back as the image, for a model to look at.
//!
//! These two choose their way out for themselves; see [`Routes`].

use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{Mutex, PoisonError},
    time::Duration,
};

use async_trait::async_trait;
use ghost_brain::{Picture, ToolSpec};
use ghost_net::{Network, Route};
use reqwest::{Client, ClientBuilder, RequestBuilder, Response, StatusCode, Url};
use serde_json::{Value, json};

use super::MAX_PICTURE;
use crate::{
    prompts::{self, Loop, Store, TEXTS},
    tool::{Outcome, Tool, text},
};

const DEEPSEEK: &str = "https://api.deepseek.com";
/// How much of a page one `fetch` returns.
const PAGE_CHARS: usize = 12_000;
/// How many of a page's images are listed after its text.
const IMAGES_LISTED: usize = 40;
/// An image a page declares narrower or shorter than this, in pixels, is
/// an icon, a spacer or a pixel that counts visits.
const MIN_SIDE: u32 = 32;

fn configured(builder: ClientBuilder) -> ClientBuilder {
    builder
        .timeout(Duration::from_secs(40))
        .connect_timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (compatible; ghost-work)")
}

/// The two ways to a site, and which one worked for each.
///
/// Her look-ups are where a proxy most often helps and most often hurts:
/// one site is reached only through it, another refuses whatever comes
/// from it. So when a proxy is configured, a request that cannot connect,
/// that times out, or that a site not yet known refuses outright (403 or
/// 451) is tried once more the other way: direct if it went by the proxy,
/// by the proxy if it went direct. The way that worked is remembered for
/// each host for as long as this process runs, and is tried first after
/// that. A site already known is not tried the other way for a refusal:
/// that refusal is the site's answer.
///
/// The model is never asked which way: it cannot see the network. Only
/// these tools switch; a model API or a platform goes the way the setting
/// says, and nothing else.
struct Routes {
    network: Network,
    direct: Client,
    proxied: Client,
    worked: Mutex<HashMap<String, Route>>,
}

/// An answer, and the way it came.
struct Reached {
    response: Response,
    route: Route,
}

/// An answer that says the site will not serve this request from where
/// it came.
fn refused(status: StatusCode) -> bool {
    matches!(status.as_u16(), 403 | 451)
}

impl Routes {
    /// Both ways, each a client `configured` the same.
    fn new(network: &Network, configured: impl Fn(ClientBuilder) -> ClientBuilder) -> Self {
        Self {
            network: network.clone(),
            direct: configured(network.builder_by(Route::Direct))
                .build()
                .unwrap_or_default(),
            proxied: configured(network.builder_by(Route::Proxy))
                .build()
                .unwrap_or_default(),
            worked: Mutex::default(),
        }
    }

    fn client(&self, route: Route) -> &Client {
        match route {
            Route::Direct => &self.direct,
            Route::Proxy => &self.proxied,
        }
    }

    fn known(&self) -> std::sync::MutexGuard<'_, HashMap<String, Route>> {
        self.worked.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Send what `request` makes, the way that works. `request` is called
    /// once for each try, with the client for that way. Why it could not
    /// be sent either way is said in `words`.
    async fn send(
        &self,
        words: &Store,
        request: impl Fn(&Client) -> RequestBuilder,
    ) -> Result<Reached, String> {
        let request = &request;
        let attempt = |route: Route| {
            let client = self.client(route);
            async move {
                let built = request(client).build()?;
                client.execute(built).await
            }
        };
        let url = request(&self.direct)
            .build()
            .map_err(|e| e.to_string())?
            .url()
            .clone();
        if !self.network.either_way(&url) {
            let route = self.network.route(&url);
            return attempt(route)
                .await
                .map(|response| Reached { response, route })
                .map_err(|e| e.to_string());
        }
        let host = url.host_str().unwrap_or_default().to_owned();
        let known = self.known().get(&host).copied();
        let first = known.unwrap_or_else(|| self.network.route(&url));
        let tried = attempt(first).await;
        let again = match &tried {
            Err(e) => e.is_connect() || e.is_timeout(),
            Ok(response) => known.is_none() && refused(response.status()),
        };
        if !again {
            if tried.as_ref().is_ok_and(|r| !refused(r.status())) {
                self.known().insert(host, first);
            }
            return tried
                .map(|response| Reached {
                    response,
                    route: first,
                })
                .map_err(|e| e.to_string());
        }
        let second = first.other();
        match (attempt(second).await, tried) {
            (Ok(response), _) if !refused(response.status()) => {
                self.known().insert(host, second);
                Ok(Reached {
                    response,
                    route: second,
                })
            }
            // Refused both ways: the first refusal is the answer.
            (_, Ok(response)) => Ok(Reached {
                response,
                route: first,
            }),
            (Ok(response), Err(_)) => Ok(Reached {
                response,
                route: second,
            }),
            (Err(again), Err(first)) => Err(words.fill(
                TEXTS,
                "unreached_either_way",
                &[("first", first.to_string()), ("again", again.to_string())],
            )),
        }
    }
}

/// Where search results come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// `POST /v1/tools/search` with the model's own key.
    DeepSeek { key: String },
    /// Brave Search API.
    Brave { key: String },
    /// A SearXNG instance with the JSON format enabled.
    Searxng { url: String },
}

/// What the web tools need, usually read from the environment.
#[derive(Debug, Clone, Default)]
pub struct WebConfig {
    pub search: Option<Source>,
    /// A DeepSeek key, if pages should be fetched through its fetch API.
    pub reader_key: Option<String>,
}

impl WebConfig {
    /// `DEEPSEEK_API_KEY`, then `BRAVE_API_KEY`, then `SEARXNG_URL`.
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let deepseek = var("DEEPSEEK_API_KEY");
        let search = deepseek
            .clone()
            .map(|key| Source::DeepSeek { key })
            .or_else(|| var("BRAVE_API_KEY").map(|key| Source::Brave { key }))
            .or_else(|| var("SEARXNG_URL").map(|url| Source::Searxng { url }));
        Self {
            search,
            reader_key: deepseek,
        }
    }
}

/// One result, whatever it came from.
struct Hit {
    title: String,
    url: String,
    about: String,
    date: String,
}

fn hits(source: &Source, v: &Value) -> Vec<Hit> {
    let s = |v: &Value| v.as_str().unwrap_or_default().trim().to_owned();
    let list = match source {
        Source::DeepSeek { .. } | Source::Searxng { .. } => v["results"].as_array(),
        Source::Brave { .. } => v["web"]["results"].as_array(),
    };
    list.map(|items| {
        items
            .iter()
            .map(|r| match source {
                Source::DeepSeek { .. } => Hit {
                    title: s(&r["title"]),
                    url: s(&r["url"]),
                    about: Some(s(&r["summary"]))
                        .filter(|x| !x.is_empty())
                        .unwrap_or_else(|| s(&r["snippet"])),
                    date: s(&r["date"]).chars().take(10).collect(),
                },
                Source::Brave { .. } => Hit {
                    title: s(&r["title"]),
                    url: s(&r["url"]),
                    about: strip_tags(&s(&r["description"])),
                    date: s(&r["age"]),
                },
                Source::Searxng { .. } => Hit {
                    title: s(&r["title"]),
                    url: s(&r["url"]),
                    about: s(&r["content"]),
                    date: s(&r["publishedDate"]).chars().take(10).collect(),
                },
            })
            .filter(|h| !h.url.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

fn listing(found: &[Hit]) -> String {
    let mut out = String::new();
    for (i, hit) in found.iter().enumerate() {
        let about: String = hit.about.chars().take(360).collect();
        let _ = write!(out, "{}. {}\n   {}", i + 1, hit.title, hit.url);
        if !hit.date.is_empty() {
            let _ = write!(out, "  ({})", hit.date);
        }
        if !about.is_empty() {
            let _ = write!(out, "\n   {}", about.replace('\n', " "));
        }
        out.push_str("\n\n");
    }
    out.trim_end().to_owned()
}

pub struct Search {
    routes: Routes,
    source: Source,
}

impl Search {
    pub fn new(source: Source) -> Self {
        Self {
            routes: Routes::new(ghost_net::current(), configured),
            source,
        }
    }

    async fn ask(&self, words: &Store, query: &str, count: u64) -> Result<(Value, Route), String> {
        let reached = self
            .routes
            .send(words, |client| match &self.source {
                Source::DeepSeek { key } => client
                    .post(format!("{DEEPSEEK}/v1/tools/search"))
                    .bearer_auth(key)
                    .json(&json!({"query": query, "count": count})),
                Source::Brave { key } => client
                    .get("https://api.search.brave.com/res/v1/web/search")
                    .header("X-Subscription-Token", key)
                    .query(&[("q", query), ("count", &count.to_string())]),
                Source::Searxng { url } => client
                    .get(format!("{}/search", url.trim_end_matches('/')))
                    .query(&[("q", query), ("format", "json")]),
            })
            .await?;
        let status = reached.response.status();
        let body = reached.response.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(words.fill(
                TEXTS,
                "search_refused",
                &[
                    ("status", status.to_string()),
                    ("said", body.chars().take(300).collect()),
                ],
            ));
        }
        serde_json::from_str(&body)
            .map(|v| (v, reached.route))
            .map_err(|e| words.fill(TEXTS, "search_unreadable", &[("why", e.to_string())]))
    }
}

#[async_trait]
impl Tool for Search {
    fn name(&self) -> &'static str {
        "search"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "search",
            json!({"query": {"type": "string"}, "count": {"type": "integer"}}),
            &["query"],
        )
    }

    async fn call(&self, args: &Value, words: &Store) -> Outcome {
        let query = match text(words, args, "query") {
            Ok(q) => q.trim(),
            Err(refusal) => return refusal,
        };
        if query.is_empty() {
            return Outcome::fail(words.text(TEXTS, "query_empty"));
        }
        // The service refuses long queries outright.
        let query: String = query.chars().take(128).collect();
        let count = args["count"].as_u64().unwrap_or(8).clamp(1, 20);
        match self.ask(words, &query, count).await {
            Ok((v, route)) => {
                let found = hits(&self.source, &v);
                let said = if found.is_empty() {
                    words.text(TEXTS, "no_results").to_owned()
                } else {
                    listing(&found)
                };
                Outcome::ok(said_with_route(words, said, route))
            }
            Err(why) => Outcome::fail(why),
        }
    }
}

/// What a tool says, and after it, if it came through the proxy, that it
/// did. The model cannot see the network; this is the one thing it is told
/// about it, because a page can differ by where it is asked from.
fn said_with_route(words: &Store, mut said: String, route: Route) -> String {
    if route == Route::Proxy {
        said.push_str(words.text(TEXTS, "by_proxy"));
    }
    said
}

pub struct Fetch {
    routes: Routes,
    reader_key: Option<String>,
    /// Where the reader is: DeepSeek, except in tests.
    reader_at: String,
}

impl Fetch {
    pub fn new(reader_key: Option<String>) -> Self {
        Self {
            routes: Routes::new(ghost_net::current(), configured),
            reader_key,
            reader_at: DEEPSEEK.to_owned(),
        }
    }

    /// The page as Markdown, through DeepSeek's fetch API.
    async fn through_reader(
        &self,
        words: &Store,
        key: &str,
        url: &str,
    ) -> Result<(String, Route), String> {
        let Reached {
            response: resp,
            route,
        } = self
            .routes
            .send(words, |client| {
                client
                    .post(format!("{}/v1/tools/fetch", self.reader_at))
                    .bearer_auth(key)
                    .json(&json!({"url": url}))
            })
            .await?;
        let status = resp.status();
        let v: Value = resp.json().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!(
                "{status}: {}",
                v["error"]["message"]
                    .as_str()
                    .unwrap_or_else(|| words.text(TEXTS, "no_reason_given"))
            ));
        }
        let title = v["title"].as_str().unwrap_or_default().trim();
        let content = v["content"].as_str().unwrap_or_default().trim();
        if content.is_empty() {
            return Err(words.text(TEXTS, "page_empty").to_owned());
        }
        let page = if title.is_empty() || content.starts_with('#') {
            content.to_owned()
        } else {
            format!("# {title}\n\n{content}")
        };
        Ok((page, route))
    }

    /// The page fetched from here and reduced to its text, with the images
    /// it shows listed after it; or the image at the address, to look at.
    async fn directly(&self, words: &Store, url: &str) -> Result<(Fetched, Route), String> {
        let Reached {
            response: mut resp,
            route,
        } = self.routes.send(words, |client| client.get(url)).await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(words.fill(TEXTS, "site_refused", &[("status", status.to_string())]));
        }
        // Relative addresses on the page are taken from where it was in
        // the end, after any redirect.
        let base = resp.url().clone();
        let kind = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_lowercase();
        // SVG is text, and is read as text.
        let maybe_image = (kind.starts_with("image/") && !kind.contains("svg"))
            || kind.contains("octet-stream")
            || kind.is_empty();
        if maybe_image {
            if resp
                .content_length()
                .is_some_and(|n| n > MAX_PICTURE as u64)
            {
                return Err(too_large(words, &kind));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
                if bytes.len() + chunk.len() > MAX_PICTURE {
                    return Err(too_large(words, &kind));
                }
                bytes.extend_from_slice(&chunk);
            }
            if let Some(picture) = super::picture(bytes.clone()) {
                return Ok((Fetched::Picture(picture), route));
            }
            if !kind.is_empty() {
                return Err(words.fill(TEXTS, "not_an_image", &[("kind", kind)]));
            }
            let body = String::from_utf8_lossy(&bytes).into_owned();
            return Ok((page_of(body, &base, false), route));
        }
        let body = resp.text().await.map_err(|e| e.to_string())?;
        if kind.contains("html")
            || kind.starts_with("text/")
            || kind.contains("json")
            || kind.contains("xml")
        {
            Ok((page_of(body, &base, kind.contains("html")), route))
        } else {
            Err(words.fill(TEXTS, "not_a_page", &[("kind", kind)]))
        }
    }
}

/// What an address held.
enum Fetched {
    /// Text to read, in parts if it is long, and the images it shows.
    Page {
        text: String,
        images: Vec<Shown>,
    },
    Picture(Picture),
}

/// A body of text as a page: HTML reduced to its words and its images,
/// anything else as it is.
fn page_of(body: String, base: &Url, html: bool) -> Fetched {
    if html || body.trim_start().starts_with('<') {
        Fetched::Page {
            text: page_text(&body),
            images: html_images(&body, base),
        }
    } else {
        Fetched::Page {
            text: body,
            images: Vec::new(),
        }
    }
}

fn too_large(words: &Store, kind: &str) -> String {
    words.fill(
        TEXTS,
        "too_large_to_show",
        &[
            (
                "kind",
                if kind.is_empty() {
                    words.text(TEXTS, "a_file").to_owned()
                } else {
                    kind.to_owned()
                },
            ),
            ("mb", (MAX_PICTURE / (1024 * 1024)).to_string()),
        ],
    )
}

/// Whether an address names an image by its extension. Such an address is
/// fetched directly: the reader makes pages into text, not images.
fn names_an_image(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp"
        )
    })
}

/// One image a page shows.
#[derive(Debug, PartialEq, Eq)]
struct Shown {
    url: String,
    /// Its alt text, or failing that its title.
    words: String,
    /// The width and height the page declares for it, as written.
    width: Option<String>,
    height: Option<String>,
}

/// The images an HTML page shows, from its `<img>` tags, in order.
fn html_images(html: &str, base: &Url) -> Vec<Shown> {
    let html = without_blocks(html, "script");
    let lower = html.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = lower[at..].find("<img") {
        let open = at + start + 4;
        let end = lower[open..].find('>').map_or(html.len(), |e| open + e);
        // `<imgfoo` is some other tag.
        if html[open..end]
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace() && c != '/')
        {
            at = end;
            continue;
        }
        let attrs = attributes(&html[open..end]);
        let get = |name: &str| {
            attrs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        // Pages that load images late keep the real address aside, and
        // put a placeholder, often a `data:` URI, in `src`.
        let src = ["data-src", "data-original", "src"]
            .iter()
            .filter_map(|name| get(name))
            .find(|v| !v.starts_with("data:"));
        if let Some(url) = src.and_then(|src| base.join(&src).ok()) {
            found.push(Shown {
                url: url.to_string(),
                words: get("alt").or_else(|| get("title")).unwrap_or_default(),
                width: get("width"),
                height: get("height"),
            });
        }
        at = end;
    }
    found
}

/// The images a Markdown page shows, `![words](address "title")`, in order.
fn markdown_images(markdown: &str, base: &Url) -> Vec<Shown> {
    let mut found = Vec::new();
    let mut rest = markdown;
    while let Some(start) = rest.find("![") {
        rest = &rest[start + 2..];
        let Some(close) = rest.find("](") else { break };
        let words = rest[..close].trim().to_owned();
        rest = &rest[close + 2..];
        let Some(end) = rest.find(')') else { break };
        let inside = rest[..end].trim();
        rest = &rest[end..];
        let address = inside.split_whitespace().next().unwrap_or_default();
        let address = address.trim_start_matches('<').trim_end_matches('>');
        if address.starts_with("data:") {
            continue;
        }
        if let Ok(url) = base.join(address) {
            found.push(Shown {
                url: url.to_string(),
                words,
                width: None,
                height: None,
            });
        }
    }
    found
}

/// The `name="value"` pairs inside a tag, names in lower case. Values may
/// be quoted either way or not at all; a bare name has an empty value.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = tag.char_indices().peekable();
    while let Some(&(i, c)) = chars.peek() {
        if c.is_whitespace() || c == '/' {
            chars.next();
            continue;
        }
        let mut name_end = i;
        while let Some(&(j, c)) = chars.peek() {
            if c.is_whitespace() || c == '=' || c == '/' {
                break;
            }
            name_end = j + c.len_utf8();
            chars.next();
        }
        let name = tag[i..name_end].to_ascii_lowercase();
        while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
            chars.next();
        }
        let mut value = String::new();
        if chars.peek().is_some_and(|(_, c)| *c == '=') {
            chars.next();
            while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                chars.next();
            }
            match chars.peek().map(|(_, c)| *c) {
                Some(q @ ('"' | '\'')) => {
                    chars.next();
                    for (_, c) in chars.by_ref() {
                        if c == q {
                            break;
                        }
                        value.push(c);
                    }
                }
                _ => {
                    while let Some(&(_, c)) = chars.peek() {
                        if c.is_whitespace() {
                            break;
                        }
                        value.push(c);
                        chars.next();
                    }
                }
            }
        }
        if !name.is_empty() {
            out.push((name, entities(&value)));
        }
    }
    out
}

fn entities(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// The images worth naming, as plain lines the model can pick from, under
/// a line in `words`: each once, at most [`IMAGES_LISTED`], and none of
/// what is only there to count visits or to be an icon: drawings in SVG,
/// and anything declared tiny.
fn listed(words: &Store, found: Vec<Shown>) -> String {
    let tiny = |side: &str| {
        side.trim_end_matches("px")
            .trim()
            .parse::<u32>()
            .is_ok_and(|n| n < MIN_SIDE)
    };
    let mut seen = std::collections::HashSet::new();
    let kept: Vec<Shown> = found
        .into_iter()
        .filter(|s| {
            let path = s.url.split(['?', '#']).next().unwrap_or_default();
            !path.to_ascii_lowercase().ends_with(".svg")
                && ![&s.width, &s.height]
                    .iter()
                    .any(|side| side.as_deref().is_some_and(tiny))
                && seen.insert(s.url.clone())
        })
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    let more = kept.len().saturating_sub(IMAGES_LISTED);
    let mut out = words.fill(TEXTS, "page_images", &[("count", kept.len().to_string())]);
    for s in kept.iter().take(IMAGES_LISTED) {
        let _ = write!(out, "- {}", s.url);
        if !s.words.is_empty() {
            let words: String = s.words.chars().take(160).collect();
            let _ = write!(
                out,
                "  \"{}\"",
                words.split_whitespace().collect::<Vec<_>>().join(" ")
            );
        }
        if let (Some(w), Some(h)) = (&s.width, &s.height) {
            let _ = write!(out, "  {w}×{h}");
        }
        out.push('\n');
    }
    if more > 0 {
        let _ = writeln!(
            out,
            "{}",
            words.fill(TEXTS, "more_images", &[("count", more.to_string())])
        );
    }
    out.trim_end().to_owned()
}

#[async_trait]
impl Tool for Fetch {
    fn name(&self) -> &'static str {
        "fetch"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "fetch",
            json!({"url": {"type": "string"}, "from": {"type": "integer"}}),
            &["url"],
        )
    }

    async fn call(&self, args: &Value, words: &Store) -> Outcome {
        let url = match text(words, args, "url") {
            Ok(u) => u.trim(),
            Err(refusal) => return refusal,
        };
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Outcome::fail(words.text(TEXTS, "not_http"));
        }
        let address = || ("url", url.to_owned());
        let fetched = match &self.reader_key {
            Some(key) if !names_an_image(url) => match self.through_reader(words, key, url).await {
                Ok((text, route)) => {
                    let mut images = Url::parse(url)
                        .map(|base| markdown_images(&text, &base))
                        .unwrap_or_default();
                    // The reader often leaves a page's images out of its
                    // text, so a page of pictures read as nothing but
                    // captions. What the page shows is then taken from
                    // the page itself.
                    if images.is_empty()
                        && let Ok((Fetched::Page { images: shown, .. }, _)) =
                            self.directly(words, url).await
                    {
                        images = shown;
                    }
                    Ok((Fetched::Page { text, images }, route))
                }
                // The reader refuses some sites that answer a plain request.
                Err(first) => self.directly(words, url).await.map_err(|second| {
                    words.fill(
                        TEXTS,
                        "page_unread_either_way",
                        &[address(), ("first", first), ("second", second)],
                    )
                }),
            },
            _ => self
                .directly(words, url)
                .await
                .map_err(|why| words.fill(TEXTS, "page_unread", &[address(), ("why", why)])),
        };
        let (page, images, route) = match fetched {
            Ok((Fetched::Page { text, images }, route)) => (text, images, route),
            Ok((Fetched::Picture(picture), route)) => {
                return Outcome::seen(
                    said_with_route(
                        words,
                        words.fill(
                            TEXTS,
                            "an_image_address",
                            &[address(), ("about", super::about(&picture))],
                        ),
                        route,
                    ),
                    picture,
                );
            }
            Err(why) => return Outcome::fail(why),
        };
        let total = page.chars().count();
        let from = usize::try_from(args["from"].as_u64().unwrap_or(0)).unwrap_or(0);
        if from >= total {
            return Outcome::fail(words.fill(
                TEXTS,
                "page_too_short",
                &[("count", total.to_string())],
            ));
        }
        let mut part: String = page.chars().skip(from).take(PAGE_CHARS).collect();
        let end = from + part.chars().count();
        // The images come with the first part, so that a long page need
        // not be read to its end to learn what it shows.
        let images = listed(words, images);
        if from == 0 && !images.is_empty() {
            let _ = write!(part, "\n\n{images}");
        }
        if end < total {
            part.push_str(&words.fill(
                TEXTS,
                "page_more",
                &[
                    ("count", (total - end).to_string()),
                    ("from", end.to_string()),
                ],
            ));
        }
        Outcome::ok(said_with_route(words, part, route))
    }
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut inside = false;
    for c in html.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

/// Remove every `<name ...>...</name>` block, case-insensitively.
fn without_blocks(html: &str, name: &str) -> String {
    let lower = html.to_lowercase();
    let (open, close) = (format!("<{name}"), format!("</{name}>"));
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while let Some(start) = lower[at..].find(&open) {
        out.push_str(&html[at..at + start]);
        match lower[at + start..].find(&close) {
            Some(end) => at += start + end + close.len(),
            None => return out,
        }
    }
    out.push_str(&html[at..]);
    out
}

/// A page's HTML as readable text: no scripts, no markup, one blank line at
/// most between blocks.
fn page_text(html: &str) -> String {
    let mut html = html.to_owned();
    for name in [
        "script", "style", "noscript", "svg", "head", "nav", "footer",
    ] {
        html = without_blocks(&html, name);
    }
    // Block-level tags become line breaks before the markup is dropped.
    let mut spaced = String::with_capacity(html.len());
    let mut rest = html.as_str();
    while let Some(at) = rest.find('<') {
        spaced.push_str(&rest[..at]);
        let tag: String = rest[at + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '/')
            .collect::<String>()
            .to_lowercase();
        if matches!(
            tag.trim_start_matches('/'),
            "p" | "br"
                | "div"
                | "li"
                | "tr"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "section"
                | "article"
                | "pre"
                | "table"
                | "ul"
                | "ol"
                | "blockquote"
        ) {
            spaced.push('\n');
        }
        spaced.push('<');
        rest = &rest[at + 1..];
    }
    spaced.push_str(rest);
    let text = strip_tags(&spaced)
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'");
    let mut out = String::new();
    let mut blank = false;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push('\n');
            blank = false;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use ghost_net::Setting;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::tools::testing::{PIXEL, serve};

    /// A proxy on this machine that refuses everything it is asked for, as
    /// one does that a site has turned away.
    async fn refusing_proxy() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0u8; 4096];
                let _ = socket.read(&mut request).await;
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
            }
        });
        format!("http://{addr}")
    }

    /// A fetch that goes by `proxy` unless `direct` lists the host, and
    /// that finds `host` at `at`.
    fn fetch_by(proxy: &str, direct: &[&str], host: &str, at: SocketAddr) -> Fetch {
        let network = Network::new(
            &Setting {
                proxy: Some(proxy.to_owned()),
                direct: Some(direct.iter().map(|&d| d.to_owned()).collect()),
            },
            |_| None,
        )
        .unwrap();
        Fetch {
            routes: Routes::new(&network, |b| configured(b).resolve(host, at)),
            reader_key: None,
            reader_at: DEEPSEEK.to_owned(),
        }
    }

    #[tokio::test]
    async fn fetch_goes_direct_when_the_proxy_is_turned_away_and_remembers_it() {
        let site = serve(vec![(
            "/harbour",
            "text/html",
            b"<p>Boats at dawn.</p>".to_vec(),
        )])
        .await;
        let at: SocketAddr = site.trim_start_matches("http://").parse().unwrap();
        let fetch = fetch_by(&refusing_proxy().await, &[], "harbour.test", at);
        let url = format!("http://harbour.test:{}/harbour", at.port());

        let read = fetch
            .call(&json!({ "url": url }), &crate::Prompts::shipped())
            .await;
        assert!(!read.failed, "{}", read.output);
        assert!(read.output.contains("Boats at dawn."), "{}", read.output);
        assert!(
            !read.output.contains("(reached through the proxy)"),
            "{}",
            read.output
        );
        assert_eq!(
            fetch.routes.known().get("harbour.test"),
            Some(&Route::Direct)
        );
        // Now known, it goes direct first.
        let again = fetch
            .call(&json!({ "url": url }), &crate::Prompts::shipped())
            .await;
        assert!(!again.failed, "{}", again.output);
    }

    #[tokio::test]
    async fn fetch_goes_by_the_proxy_when_a_site_cannot_be_reached_directly_and_says_so() {
        // Nothing listens here, so going direct cannot connect.
        let closed = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let url = format!("http://far.test:{}/page", closed.port());
        // A proxy is asked for the whole address.
        let path: &'static str = Box::leak(url.clone().into_boxed_str());
        let proxy = serve(vec![(path, "text/html", b"<p>Far away.</p>".to_vec())]).await;
        let fetch = fetch_by(&proxy, &["far.test"], "far.test", closed);

        let read = fetch
            .call(&json!({ "url": url }), &crate::Prompts::shipped())
            .await;
        assert!(!read.failed, "{}", read.output);
        assert!(read.output.contains("Far away."), "{}", read.output);
        assert!(
            read.output.ends_with("(reached through the proxy)"),
            "{}",
            read.output
        );
        assert_eq!(fetch.routes.known().get("far.test"), Some(&Route::Proxy));
        let again = fetch
            .call(&json!({ "url": url }), &crate::Prompts::shipped())
            .await;
        assert!(
            again.output.ends_with("(reached through the proxy)"),
            "{}",
            again.output
        );
    }

    #[tokio::test]
    async fn fetching_an_image_returns_the_picture_whatever_type_it_is_served_as() {
        let base = serve(vec![
            ("/a.png", "image/png", PIXEL.to_vec()),
            ("/raw", "application/octet-stream", PIXEL.to_vec()),
            (
                "/fake.png",
                "image/png",
                b"<html>not an image</html>".to_vec(),
            ),
        ])
        .await;
        let fetch = Fetch::new(None);
        for path in ["/a.png", "/raw"] {
            let seen = fetch
                .call(
                    &json!({"url": format!("{base}{path}")}),
                    &crate::Prompts::shipped(),
                )
                .await;
            assert!(!seen.failed, "{}", seen.output);
            assert_eq!(seen.pictures.len(), 1);
            assert_eq!(seen.pictures[0].mime, "image/png");
            assert_eq!(seen.pictures[0].bytes, PIXEL);
            assert!(
                seen.output.contains("image/png") && seen.output.contains("1×1"),
                "{}",
                seen.output
            );
        }
        // What only claims to be an image is not shown as one.
        let fake = fetch
            .call(
                &json!({"url": format!("{base}/fake.png")}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert!(fake.failed && fake.pictures.is_empty(), "{}", fake.output);
    }

    #[tokio::test]
    async fn a_page_lists_the_images_it_shows_after_its_text() {
        let page = r#"<html><body><h1>Harbour</h1><p>Boats at dawn.</p>
            <img src="/boats.jpg" alt="Boats at  dawn" width="800" height="600">
            <img src='pix.gif' width=1 height=1>
            <img src="/logo.svg" alt="logo">
            <img data-src="https://cdn.example/late.webp" src="data:image/gif;base64,R0lGOD" title="Loaded late">
            <img src="/boats.jpg" alt="again">
            </body></html>"#;
        let base = serve(vec![(
            "/news/today",
            "text/html; charset=utf-8",
            page.into(),
        )])
        .await;
        let read = Fetch::new(None)
            .call(
                &json!({"url": format!("{base}/news/today")}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert!(!read.failed, "{}", read.output);
        assert!(read.pictures.is_empty(), "a page is read, not shown");
        let (text, images) = read.output.split_once("\n\nImages on this page").unwrap();
        assert!(text.contains("Boats at dawn."));
        assert_eq!(
            images,
            format!(
                " (2). Fetch an address to see the image:\n\
                 - {base}/boats.jpg  \"Boats at dawn\"  800×600\n\
                 - https://cdn.example/late.webp  \"Loaded late\""
            )
        );
    }

    #[tokio::test]
    async fn a_page_read_without_its_images_still_lists_what_it_shows() {
        // The reader gives the captions of a gallery and none of its
        // pictures; the page itself has them, one loaded late.
        let page = r#"<html><body><h1>Possums</h1>
            <img src="/grey.jpg" data-original="/a1.gif" alt="received, cannot do">
            <img src="/a2.webp" alt="deep thought"></body></html>"#;
        let reader =
            r##"{"title":"Possums","content":"# Possums\n\nreceived, cannot do\ndeep thought"}"##;
        let base = serve(vec![
            ("/gallery", "text/html; charset=utf-8", page.into()),
            ("/v1/tools/fetch", "application/json", reader.into()),
        ])
        .await;
        let mut fetch = Fetch::new(Some("key".into()));
        fetch.reader_at.clone_from(&base);
        let read = fetch
            .call(
                &json!({"url": format!("{base}/gallery")}),
                &crate::Prompts::shipped(),
            )
            .await;
        assert!(!read.failed, "{}", read.output);
        let (text, images) = read.output.split_once("\n\nImages on this page").unwrap();
        assert!(text.contains("deep thought"), "{text}");
        assert!(
            images.contains(&format!("{base}/a1.gif  \"received, cannot do\""))
                && images.contains(&format!("{base}/a2.webp")),
            "{images}"
        );
    }

    #[test]
    fn images_in_markdown_are_found_with_their_words() {
        let base = Url::parse("https://example.com/a/b").unwrap();
        let found = markdown_images(
            "Intro ![A cat](c.jpg \"t\") and ![](https://x.example/d.png) ![no](data:x)",
            &base,
        );
        assert_eq!(
            found
                .iter()
                .map(|s| (s.url.as_str(), s.words.as_str()))
                .collect::<Vec<_>>(),
            [
                ("https://example.com/a/c.jpg", "A cat"),
                ("https://x.example/d.png", "")
            ]
        );
    }

    #[test]
    fn only_so_many_images_are_listed() {
        let found = (0..50)
            .map(|i| Shown {
                url: format!("https://example.com/{i}.jpg"),
                words: String::new(),
                width: None,
                height: None,
            })
            .collect();
        let list = listed(&crate::Prompts::shipped(), found);
        assert_eq!(list.lines().filter(|l| l.starts_with("- ")).count(), 40);
        assert!(list.ends_with("[and 10 more]"), "{list}");
    }

    #[test]
    fn a_page_is_reduced_to_its_words() {
        let html = "<html><head><title>T</title><style>p{}</style></head><body>\
            <nav>menu</nav><h1>Old  streets</h1><p>Facades that <b>lean</b> a little.</p>\
            <script>var x = '<p>no</p>';</script><ul><li>one</li><li>two &amp; three</li></ul></body></html>";
        assert_eq!(
            page_text(html),
            "Old streets\n\nFacades that lean a little.\n\none\n\ntwo & three"
        );
    }

    #[test]
    fn results_from_each_source_read_the_same() {
        let deepseek = json!({"results": [{"title": "A", "url": "https://a.example", "summary": "about a", "snippet": "x", "date": "2026-06-01T00:00:00+00:00"}]});
        let brave = json!({"web": {"results": [{"title": "A", "url": "https://a.example", "description": "about <strong>a</strong>", "age": "2 days ago"}]}});
        let searx =
            json!({"results": [{"title": "A", "url": "https://a.example", "content": "about a"}]});
        let one = listing(&hits(&Source::DeepSeek { key: String::new() }, &deepseek));
        assert_eq!(one, "1. A\n   https://a.example  (2026-06-01)\n   about a");
        let two = listing(&hits(&Source::Brave { key: String::new() }, &brave));
        assert_eq!(two, "1. A\n   https://a.example  (2 days ago)\n   about a");
        let three = listing(&hits(&Source::Searxng { url: String::new() }, &searx));
        assert_eq!(three, "1. A\n   https://a.example\n   about a");
    }
}
