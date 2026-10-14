//! The WHATWG `URL` the embeds read and write: `new URL(input)`, its `hostname`, `pathname`,
//! `protocol` and `searchParams.get`, and the URL `searchParams.set` makes of a base.

use url::Url;

/// `new URL(input)`, which throws where the input isn't an absolute URL.
pub(crate) struct WebUrl(Url);

impl WebUrl {
    pub fn parse(input: &str) -> Option<WebUrl> {
        Url::parse(input).ok().map(WebUrl)
    }

    pub fn hostname(&self) -> &str {
        self.0.host_str().unwrap_or_default()
    }

    pub fn pathname(&self) -> &str {
        self.0.path()
    }

    pub fn protocol(&self) -> String {
        format!("{}:", self.0.scheme())
    }

    /// `searchParams.get(name)`: the first value of the name in the query.
    pub fn search_param(&self, name: &str) -> Option<String> {
        self.0
            .query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    }
}

/// `new URL(base)` with `searchParams.set(name, value)` of each distinct name in turn, as its
/// `toString()` writes it.
pub(crate) fn with_search_params(base: &str, params: &[(&str, &str)]) -> String {
    let mut url = Url::parse(base).expect("a URL");
    url.query_pairs_mut().extend_pairs(params);
    url.into()
}
