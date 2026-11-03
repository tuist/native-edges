//! The navigation catalog. Every screen is declared here, so the edges, the
//! CLI, and agents can discover them, and every screen has a URL that opens it.

use serde::Serialize;
use std::collections::BTreeMap;

pub const SCHEME: &str = "edges";

/// A screen the edges know how to render.
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
        id: "projects",
        title: "Projects",
        description: "The Tuist projects the signed-in user can access",
        path: "projects",
        parent: None,
    },
    Route {
        id: "project",
        title: "Project",
        description: "One Tuist project, identified by its account and project handles",
        path: "projects/{account}/{project}",
        parent: Some("projects"),
    },
];

pub type Params = BTreeMap<String, String>;

/// One entry in a navigation stack.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Screen {
    pub route: &'static str,
    pub params: Params,
    pub url: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    UnknownRoute,
    InvalidUrl,
    MissingParam(&'static str),
}

/// Resolves a deep link into the stack of screens to show, root first.
pub fn open(url: &str) -> Result<Vec<Screen>, Error> {
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

fn stack(route: &'static Route, params: Params) -> Result<Vec<Screen>, Error> {
    let mut screens = Vec::new();
    let mut current = Some(route);
    while let Some(route) = current {
        let own: Params = route
            .path
            .split('/')
            .filter_map(parameter)
            .filter_map(|name| params.get(name).map(|v| (name.to_string(), v.clone())))
            .collect();
        screens.push(Screen {
            route: route.id,
            url: url(route.id, &own)?,
            params: own,
        });
        current = route.parent.map(find).transpose()?;
    }
    screens.reverse();
    Ok(screens)
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

/// Tuist account and project handles: letters, digits, `-`, `_`, and `.`.
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
    fn a_project_link_opens_on_top_of_the_project_list() {
        let stack = open("edges://projects/tuist/tuist?utm=x").unwrap();
        assert_eq!(
            stack,
            vec![
                Screen {
                    route: "projects",
                    params: Params::new(),
                    url: "edges://projects".into()
                },
                Screen {
                    route: "project",
                    params: params(&[("account", "tuist"), ("project", "tuist")]),
                    url: "edges://projects/tuist/tuist".into()
                },
            ]
        );
    }

    #[test]
    fn links_round_trip_through_url() {
        let p = params(&[("account", "acme"), ("project", "ios-app")]);
        let link = url("project", &p).unwrap();
        assert_eq!(link, "edges://projects/acme/ios-app");
        assert_eq!(open(&link).unwrap().last().unwrap().params, p);
    }

    #[test]
    fn rejects_unknown_and_malformed_links() {
        assert_eq!(open("https://tuist.dev/projects"), Err(Error::InvalidUrl));
        assert_eq!(open("edges://settings"), Err(Error::UnknownRoute));
        assert_eq!(open("edges://projects/tuist"), Err(Error::UnknownRoute));
        assert_eq!(open("edges://projects/../etc"), Err(Error::UnknownRoute));
        assert_eq!(
            url("project", &params(&[("account", "a b"), ("project", "x")])),
            Err(Error::MissingParam("account"))
        );
    }
}
