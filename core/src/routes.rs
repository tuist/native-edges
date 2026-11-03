//! The navigation catalog. Every destination is declared here, so the edges,
//! the CLI, and agents can discover them, and each one has a URL that opens it.

use serde::Serialize;
use std::collections::HashMap;

pub const SCHEME: &str = "edges";

/// A destination the edges know how to present.
#[derive(Debug, Serialize)]
pub struct Route {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Path segments after `edges://`. `{name}` segments are parameters.
    pub path: &'static str,
    /// The route shown underneath this one, so a deep link builds a full back stack.
    pub parent: Option<&'static str>,
}

pub static ROUTES: &[Route] = &[
    Route {
        id: "conversations",
        title: "Conversations",
        description: "Your conversations with ChatGPT, stored on this device",
        path: "conversations",
        parent: None,
    },
    // Declared before `conversation` so `new` isn't read as an ID.
    Route {
        id: "new",
        title: "New conversation",
        description: "Start a conversation with ChatGPT",
        path: "conversations/new",
        parent: Some("conversations"),
    },
    Route {
        id: "conversation",
        title: "Conversation",
        description: "One conversation, identified by its ID",
        path: "conversations/{id}",
        parent: Some("conversations"),
    },
];

pub type Params = HashMap<String, String>;

/// One entry in a navigation stack: where the person is, not how it looks.
#[derive(Debug, PartialEq, Eq, uniffi::Record)]
pub struct Destination {
    pub route: String,
    pub params: Params,
    pub url: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    UnknownRoute,
    InvalidUrl,
    MissingParam(&'static str),
}

/// Resolves a deep link into the stack of destinations, root first.
pub fn open(url: &str) -> Result<Vec<Destination>, Error> {
    let rest = url
        .strip_prefix(SCHEME)
        .and_then(|rest| rest.strip_prefix("://"))
        .ok_or(Error::InvalidUrl)?;
    let path = rest.split(['?', '#']).next().unwrap_or_default();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    for route in ROUTES {
        if let Some(params) = matches(route, &segments) {
            return stack(route, params);
        }
    }
    Err(Error::UnknownRoute)
}

/// Builds the deep link for a route.
pub fn url(route_id: &str, params: &Params) -> Result<String, Error> {
    let route = find(route_id)?;
    let mut segments = Vec::new();
    for segment in route.path.split('/') {
        match parameter(segment) {
            Some(name) => {
                let value = params
                    .get(name)
                    .filter(|value| is_handle(value))
                    .ok_or(Error::MissingParam(static_name(route, name)))?;
                segments.push(value.as_str());
            }
            None => segments.push(segment),
        }
    }
    Ok(format!("{SCHEME}://{}", segments.join("/")))
}

fn stack(route: &'static Route, params: Params) -> Result<Vec<Destination>, Error> {
    let mut destinations = Vec::new();
    let mut current = Some(route);
    while let Some(route) = current {
        let own: Params = route
            .path
            .split('/')
            .filter_map(parameter)
            .filter_map(|name| params.get(name).map(|v| (name.to_string(), v.clone())))
            .collect();
        destinations.push(Destination {
            route: route.id.into(),
            url: url(route.id, &own)?,
            params: own,
        });
        current = route.parent.map(find).transpose()?;
    }
    destinations.reverse();
    Ok(destinations)
}

fn matches(route: &Route, segments: &[&str]) -> Option<Params> {
    let pattern: Vec<&str> = route.path.split('/').collect();
    if pattern.len() != segments.len() {
        return None;
    }
    let mut params = Params::new();
    for (expected, actual) in pattern.iter().zip(segments) {
        match parameter(expected) {
            Some(name) if is_handle(actual) => {
                params.insert(name.to_string(), (*actual).to_string());
            }
            Some(_) => return None,
            None if expected == actual => {}
            None => return None,
        }
    }
    Some(params)
}

fn find(id: &str) -> Result<&'static Route, Error> {
    ROUTES
        .iter()
        .find(|route| route.id == id)
        .ok_or(Error::UnknownRoute)
}

fn parameter(segment: &str) -> Option<&str> {
    segment.strip_prefix('{')?.strip_suffix('}')
}

fn static_name(route: &'static Route, name: &str) -> &'static str {
    route
        .path
        .split('/')
        .filter_map(parameter)
        .find(|candidate| *candidate == name)
        .unwrap_or("param")
}

/// Route parameters: letters, digits, `-`, `_`, and `.`.
fn is_handle(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && value != "."
        && value != ".."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Params {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_conversation_link_opens_on_top_of_the_list() {
        let stack = open("edges://conversations/3f2a9c1b?utm=x").unwrap();
        assert_eq!(
            stack,
            vec![
                Destination {
                    route: "conversations".into(),
                    params: Params::new(),
                    url: "edges://conversations".into()
                },
                Destination {
                    route: "conversation".into(),
                    params: params(&[("id", "3f2a9c1b")]),
                    url: "edges://conversations/3f2a9c1b".into()
                },
            ]
        );
        assert_eq!(open("edges://conversations/new").unwrap()[1].route, "new");
    }

    #[test]
    fn links_round_trip_through_url() {
        let p = params(&[("id", "ab12")]);
        let link = url("conversation", &p).unwrap();
        assert_eq!(link, "edges://conversations/ab12");
        assert_eq!(open(&link).unwrap().last().unwrap().params, p);
    }

    #[test]
    fn rejects_unknown_and_malformed_links() {
        assert_eq!(
            open("https://chatgpt.com/conversations"),
            Err(Error::InvalidUrl)
        );
        assert_eq!(open("edges://settings"), Err(Error::UnknownRoute));
        assert_eq!(open("edges://conversations/a/b"), Err(Error::UnknownRoute));
        assert_eq!(open("edges://conversations/.."), Err(Error::UnknownRoute));
        assert_eq!(
            url("conversation", &params(&[("id", "a b")])),
            Err(Error::MissingParam("id"))
        );
    }
}
