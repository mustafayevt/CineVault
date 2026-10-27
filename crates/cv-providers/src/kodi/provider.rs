//! The provider that runs one XML scraper.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use cv_domain::{
    EpisodeGroup, EpisodeOrdering, Item, ItemDetail, MediaFileType, ProviderIds, id_key,
};

use super::addon::{KodiAddon, ScraperEntry};
use super::engine::Interpreter;
use super::results::{self, Reference};
use super::scraper::{KodiContent, KodiError, KodiSettings, ScraperDef, ScraperUrl};
use super::text::url_encode;
use super::xml::{self, Element, Node};
use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, ProviderLimits};
use crate::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use crate::traits::{MetadataProvider, Provider};

/// How many times a result may hand over to a chained function.
const MAX_CHAIN_ROUNDS: usize = 8;

const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Provider IDs and names live for the whole run; this keeps one copy of each.
fn intern(text: &str) -> &'static str {
    static POOL: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut pool = POOL
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = pool.get(text) {
        return existing;
    }
    let leaked: &'static str = Box::leak(text.to_owned().into_boxed_str());
    pool.insert(leaked);
    leaked
}

/// How fast a scraper's pages are fetched: two at a time and one per second.
pub fn default_limits(name: &str) -> ProviderLimits {
    ProviderLimits::new(2, 1, Duration::from_secs(1)).named(name)
}

/// Settings of the provider that are not the scraper's own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KodiConfig {
    /// Run the scraper's `GetEpisodeDetails` for every episode of a list. Off by default: it
    /// costs one request per episode.
    pub episode_details: bool,
}

/// A provider backed by one Kodi XML scraper. Cloning is cheap.
#[derive(Clone)]
pub struct KodiProvider {
    http: HttpClient,
    key: &'static str,
    name: &'static str,
    def: Arc<ScraperDef>,
    settings: Arc<KodiSettings>,
    content: KodiContent,
    config: KodiConfig,
}

impl KodiProvider {
    /// A provider for `def`. `key` is the provider key (the add-on ID), `name` the name to
    /// show. The polite [`default_limits`] are set on the provider's HTTP lane.
    pub fn new(
        http: HttpClient,
        key: &str,
        name: &str,
        def: ScraperDef,
        settings: KodiSettings,
        content: KodiContent,
    ) -> Self {
        let provider = Self {
            http,
            key: intern(key),
            name: intern(name),
            def: Arc::new(def),
            settings: Arc::new(settings),
            content,
            config: KodiConfig::default(),
        };
        provider
            .http
            .set_provider_limits(provider.key, default_limits(provider.name));
        provider
    }

    /// A provider for a scraper of a discovered add-on. `overrides` are the user's values for
    /// the add-on's settings; they win over the add-on's defaults.
    pub fn from_addon(
        http: HttpClient,
        addon: &KodiAddon,
        entry: &ScraperEntry,
        key: &str,
        overrides: &KodiSettings,
    ) -> Result<Self, KodiError> {
        let (def, defaults) = addon.load(entry)?;
        Ok(Self::new(
            http,
            key,
            &addon.name,
            def,
            defaults.with_overrides(overrides),
            entry.content,
        ))
    }

    #[must_use]
    pub fn with_config(mut self, config: KodiConfig) -> Self {
        self.config = config;
        self
    }

    pub fn content(&self) -> KodiContent {
        self.content
    }

    fn kind(&self) -> MediaKind {
        match self.content {
            KodiContent::Movies => MediaKind::Movie,
            KodiContent::TvShows => MediaKind::Show,
        }
    }

    fn error(&self, error: KodiError) -> ProviderError {
        match error {
            KodiError::MissingFunction(function) => ProviderError::Unsupported {
                provider: self.name.to_owned(),
                operation: match function.as_str() {
                    "CreateSearchUrl" | "GetSearchResults" => "searching",
                    "GetDetails" => "details",
                    "GetEpisodeList" => "episode lists",
                    "GetEpisodeDetails" => "episode details",
                    _ => "this scraper function",
                },
            },
            other => ProviderError::Other(format!("{}: {other}", self.name)),
        }
    }

    /// Fetches one page a scraper asked for.
    async fn fetch(
        &self,
        url: &ScraperUrl,
        options: &RequestOptions,
    ) -> Result<String, ProviderError> {
        url.check().map_err(|e| self.error(e))?;
        let mut request = if url.post {
            let (base, body) = url.url.split_once('?').unwrap_or((&url.url, ""));
            HttpRequest::post(self.key, base, body.as_bytes().to_vec())
                .header("Content-Type", "application/x-www-form-urlencoded")
        } else {
            HttpRequest::get(self.key, &url.url).html()
        };
        request = request
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Accept-Language", &options.language)
            .bypass_cache(options.bypass_cache);
        if let Some(spoof) = &url.spoof {
            request = request.header("Referer", spoof);
        }
        let response = self.http.send(request).await?;
        match response.status() {
            404 | 410 => Err(ProviderError::NotFound(self.name.to_owned())),
            _ if !response.is_success() => Err(ProviderError::Request {
                provider: self.name.to_owned(),
                message: format!("HTTP {}", response.status()),
            }),
            _ => Ok(response.text()),
        }
    }

    /// Runs a scraper function: the pages of `urls` go into buffers 1, 2, ..., `extras` follow,
    /// and chained functions in the answer are resolved.
    async fn call(
        &self,
        interpreter: &mut Interpreter<'_>,
        function: &str,
        urls: &[ScraperUrl],
        extras: &[String],
        options: &RequestOptions,
    ) -> Result<String, ProviderError> {
        if !self.def.has_function(function) {
            return Err(self.error(KodiError::MissingFunction(function.to_owned())));
        }
        let mut inputs: Vec<String> = Vec::with_capacity(urls.len() + extras.len());
        for url in urls {
            inputs.push(self.fetch(url, options).await?);
        }
        inputs.extend(extras.iter().cloned());
        interpreter.set_inputs(&inputs);
        let answer = interpreter.run(function).map_err(|e| self.error(e))?;
        Ok(resolve_chains(interpreter, answer))
    }

    /// Reads an NFO file's text with the scraper's `NfoUrl` function: the title it points at,
    /// if the scraper recognises the text. `Ok(None)` when it does not (or has no such
    /// function).
    pub async fn nfo_url(&self, nfo_text: &str) -> Result<Option<Reference>, ProviderError> {
        if !self.def.has_function("NfoUrl") {
            return Ok(None);
        }
        let mut interpreter = Interpreter::new(&self.def, &self.settings);
        interpreter.set_inputs(&[nfo_text.to_owned()]);
        let answer = interpreter.run("NfoUrl").map_err(|e| self.error(e))?;
        let document = xml::parse(&answer);
        let reference = Reference {
            id: document.child_text("id"),
            urls: ScraperUrl::parse_all(&answer),
        };
        Ok((!reference.id.is_empty() || !reference.urls.is_empty()).then_some(reference))
    }

    /// The key to read a title by: the add-on's own ID if `ids` has one, else the title
    /// the scraper's `NfoUrl` finds for the IMDb ID.
    async fn reference_for(
        &self,
        ids: &ProviderIds,
    ) -> Result<Option<(String, Reference)>, ProviderError> {
        if let Some(text) = ids.get(self.key) {
            return Ok(Some((text.to_owned(), Reference::decode(text))));
        }
        if let Some(imdb) = ids.get(id_key::IMDB)
            && let Some(reference) = self
                .nfo_url(&format!("https://www.imdb.com/title/{imdb}/"))
                .await?
        {
            return Ok(Some((reference.encode(), reference)));
        }
        Ok(None)
    }

    async fn details_of(
        &self,
        interpreter: &mut Interpreter<'_>,
        reference: &Reference,
        options: &RequestOptions,
    ) -> Result<String, ProviderError> {
        let function = reference
            .urls
            .first()
            .and_then(|u| u.function.as_deref())
            .unwrap_or("GetDetails");
        let answer = self
            .call(
                interpreter,
                function,
                &reference.urls,
                std::slice::from_ref(&reference.id),
                options,
            )
            .await?;
        if results::is_empty_answer(&answer) {
            return Err(ProviderError::NotFound(self.name.to_owned()));
        }
        Ok(answer)
    }

    fn hit_of(&self, item: &Item) -> SearchHit {
        SearchHit {
            provider: self.key.to_owned(),
            ids: item.core.ids.clone(),
            title: item.core.title.clone(),
            original_title: item.core.original_title.clone(),
            year: item.core.year,
            overview: item.core.plot.clone(),
            poster_url: item.core.artwork_urls.get(&MediaFileType::Poster).cloned(),
            exact_id: true,
            popularity: 0.0,
            votes: 0,
        }
    }

    /// Reads one episode's details with `GetEpisodeDetails`.
    pub async fn episode_details(
        &self,
        episode: &Item,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let text = episode
            .core
            .ids
            .get(self.key)
            .ok_or_else(|| ProviderError::NotFound(self.name.to_owned()))?;
        let reference = Reference::decode(text);
        let mut interpreter = Interpreter::new(&self.def, &self.settings);
        let answer = self
            .call(
                &mut interpreter,
                "GetEpisodeDetails",
                &reference.urls,
                std::slice::from_ref(&reference.id),
                options,
            )
            .await?;
        let document = xml::parse(&answer);
        let element = document.first("details").unwrap_or(&document);
        let mut detailed = results::episode_item(element, self.key);
        if detailed.core.ids.get(self.key).is_none() {
            detailed.core.ids.set(self.key, text);
        }
        if let (ItemDetail::Episode(from), ItemDetail::Episode(to)) =
            (&episode.detail, &mut detailed.detail)
        {
            if to.numbers.get(&EpisodeGroup::aired()).is_none() {
                to.numbers = from.numbers.clone();
            }
            if to.first_aired.is_none() {
                to.first_aired = from.first_aired;
            }
        }
        if detailed.core.title.is_empty() {
            detailed.core.title.clone_from(&episode.core.title);
        }
        Ok(detailed)
    }
}

/// Replaces `<chain function="F">param</chain>` elements in an answer by what `F` returns for
/// `param` (the children of its answer element). A chain that fails is dropped.
fn resolve_chains(interpreter: &mut Interpreter<'_>, mut answer: String) -> String {
    for _ in 0..MAX_CHAIN_ROUNDS {
        if !answer.contains("<chain") {
            break;
        }
        let mut document = xml::parse(&answer);
        let Some(root) = document.children.iter_mut().find_map(|node| match node {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        }) else {
            break;
        };
        let mut changed = false;
        let mut children = Vec::with_capacity(root.children.len());
        for node in std::mem::take(&mut root.children) {
            match node {
                Node::Element(chain) if chain.name == "chain" => {
                    changed = true;
                    let Some(function) = chain.attr("function") else {
                        continue;
                    };
                    interpreter.set_inputs(&[chain.text().trim().to_owned()]);
                    match interpreter.run(function) {
                        Ok(output) => {
                            let parsed = xml::parse(&output);
                            let source: &Element = parsed
                                .child_elements()
                                .next()
                                .filter(|_| parsed.child_elements().count() == 1)
                                .unwrap_or(&parsed);
                            children.extend(source.children.iter().cloned());
                        }
                        Err(error) => {
                            tracing::warn!(%error, function, "a chained scraper function failed");
                        }
                    }
                }
                other => children.push(other),
            }
        }
        root.children = children;
        if !changed {
            break;
        }
        answer = document.children.iter().map(node_xml).collect();
    }
    answer
}

fn node_xml(node: &Node) -> String {
    match node {
        Node::Element(e) => e.to_xml(),
        Node::Text(t) => xml::escape(t),
    }
}

#[async_trait]
impl Provider for KodiProvider {
    fn id(&self) -> &'static str {
        self.key
    }

    fn name(&self) -> &'static str {
        self.name
    }
}

#[async_trait]
impl MetadataProvider for KodiProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: self.content == KodiContent::Movies,
            shows: self.content == KodiContent::TvShows,
            orderings: if self.content == KodiContent::TvShows {
                vec![EpisodeOrdering::Aired]
            } else {
                Vec::new()
            },
            lookup_ids: vec![self.key],
            best_effort: false,
        }
    }

    async fn search(
        &self,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        if query.kind != self.kind() {
            return Ok(Vec::new());
        }
        let year = query.year.map(|y| y.to_string()).unwrap_or_default();
        let mut interpreter = Interpreter::new(&self.def, &self.settings);
        let search_url = self
            .call(
                &mut interpreter,
                "CreateSearchUrl",
                &[],
                &[url_encode(&query.title), year.clone()],
                options,
            )
            .await?;
        let urls = ScraperUrl::parse_all(&search_url);
        if urls.is_empty() {
            return Ok(Vec::new());
        }
        let answer = self
            .call(
                &mut interpreter,
                "GetSearchResults",
                &urls,
                &[query.title.clone(), year],
                options,
            )
            .await?;
        Ok(results::search_hits(&answer, self.key))
    }

    async fn lookup(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        if kind != self.kind() {
            return Ok(None);
        }
        match self.details(kind, ids, options).await {
            Ok(item) => Ok(Some(self.hit_of(&item))),
            Err(ProviderError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn details(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        if kind != self.kind() {
            return Err(ProviderError::Unsupported {
                provider: self.name.to_owned(),
                operation: "this kind of title",
            });
        }
        let Some((text, reference)) = self.reference_for(ids).await? else {
            return Err(ProviderError::NotFound(self.name.to_owned()));
        };
        let mut interpreter = Interpreter::new(&self.def, &self.settings);
        let answer = self
            .details_of(&mut interpreter, &reference, options)
            .await?;
        let item = results::details_item(&answer, self.content, self.key, &text, &options.country);
        // A page the scraper could not read gives no title. Nothing is returned, so a caller
        // never stores a half-filled title.
        if item.core.title.trim().is_empty() {
            return Err(ProviderError::FormatChanged(self.name.to_owned()));
        }
        Ok(item)
    }

    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        if self.content != KodiContent::TvShows {
            return Err(ProviderError::Unsupported {
                provider: self.name.to_owned(),
                operation: "episode lists",
            });
        }
        if ordering.ordering != EpisodeOrdering::Aired {
            return Err(ProviderError::NotFound(self.name.to_owned()));
        }
        let Some((_, reference)) = self.reference_for(show).await? else {
            return Err(ProviderError::NotFound(self.name.to_owned()));
        };
        let mut interpreter = Interpreter::new(&self.def, &self.settings);
        let details = self
            .details_of(&mut interpreter, &reference, options)
            .await?;
        let mut guide = results::episode_guide_urls(&details);
        if guide.is_empty() {
            guide.clone_from(&reference.urls);
        }
        let function = guide
            .first()
            .and_then(|u| u.function.clone())
            .unwrap_or_else(|| "GetEpisodeList".to_owned());
        let answer = self
            .call(
                &mut interpreter,
                &function,
                &guide,
                std::slice::from_ref(&reference.id),
                options,
            )
            .await?;
        let mut episodes = results::episode_list(&answer, self.key);
        if episodes.is_empty() {
            return Err(ProviderError::NotFound(self.name.to_owned()));
        }
        if self.config.episode_details && self.def.has_function("GetEpisodeDetails") {
            for episode in &mut episodes {
                if episode.core.ids.get(self.key).is_none() {
                    continue;
                }
                match self.episode_details(episode, options).await {
                    Ok(detailed) => *episode = detailed,
                    Err(error) => {
                        tracing::debug!(%error, "episode details unavailable; keeping the list entry");
                    }
                }
            }
        }
        Ok(episodes)
    }
}
