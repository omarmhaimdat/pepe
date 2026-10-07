//! API mode: a spec, its plan, and how to turn the plan into load

use crate::cli::{ApiArgs, Cli};
use crate::load::Target;
use crate::openapi::{self, Credentials, Endpoint, PlanOptions, Spec};
use crate::request::{parse_header, ConnectTimes, Request};
use crate::ui::EndpointView;
use crate::PepeError;

/// Weights are whole numbers; this keeps an endpoint's weight divisible
/// among its URL variants
const VARIANT_SCALE: u32 = 64;

pub struct ApiRun {
    pub spec: Spec,
    pub options: PlanOptions,
    pub credentials: Credentials,
    pub endpoints: Vec<Endpoint>,
}

impl ApiRun {
    /// Read the spec and plan it with the flags given
    pub async fn load(api: &ApiArgs) -> Result<Self, String> {
        let doc = openapi::load(&api.spec).await?;
        // A spec fetched over http says where the API is when it names no server
        let origin = api.spec.contains("://").then_some(api.spec.as_str());
        let spec = Spec::parse(&doc, origin, api.server.as_deref())?;
        let credentials = Credentials::parse(&api.auth, &spec)?;
        let options = PlanOptions {
            only: api.only.clone(),
            tags: api.tag.clone(),
            skip: api.skip.clone(),
            set: PlanOptions::parse_set(&api.set)?,
            all: api.all,
            include_writes: api.include_writes,
        };
        let endpoints = openapi::plan(&spec, &options);
        if endpoints.is_empty() {
            let tags: Vec<&str> = spec.tags.iter().map(|(name, _)| name.as_str()).collect();
            return Err(format!(
                "no endpoint of the spec matches --tag/--only/--skip (its tags: {})",
                tags.join(", ")
            ));
        }
        Ok(ApiRun {
            spec,
            options,
            credentials,
            endpoints,
        })
    }

    /// The URLs an endpoint's requests go to
    pub fn urls(&self, index: usize) -> Vec<String> {
        self.endpoints[index].urls(&self.spec.base_url, &self.credentials)
    }

    /// Indexes of the endpoints that are switched on
    pub fn enabled(&self) -> Vec<usize> {
        (0..self.endpoints.len())
            .filter(|&i| self.endpoints[i].enabled)
            .collect()
    }

    /// Headers every request carries: -H flags, then credentials
    fn shared_headers(&self, cli: &Cli) -> Vec<String> {
        let mut headers = cli.headers.clone();
        headers.extend(
            self.credentials
                .headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}")),
        );
        headers
    }

    /// One client per load shard, and where their connection times go
    pub fn clients(
        &self,
        cli: &Cli,
        shards: crate::load::Threads,
    ) -> Result<(crate::load::Senders, std::sync::Arc<ConnectTimes>), PepeError> {
        self.shared_request(cli)?.build_clients(shards)
    }

    pub fn client(&self, cli: &Cli) -> Result<crate::request::Sender, PepeError> {
        self.shared_request(cli)?.build_client()
    }

    /// The request every endpoint builds on: the base URL and shared headers
    fn shared_request(&self, cli: &Cli) -> Result<Request, PepeError> {
        Request::new(
            self.spec.base_url.clone(),
            "GET".into(),
            None,
            &self.shared_headers(cli),
            cli.settings(),
        )
    }

    /// The requests for these endpoints; results are tagged with the
    /// endpoint's position in `which`
    pub fn targets(&self, cli: &Cli, which: &[usize]) -> Result<Vec<Target>, PepeError> {
        let mut targets = Vec::new();
        for (tag, &index) in which.iter().enumerate() {
            let endpoint = &self.endpoints[index];
            let mut headers = reqwest::header::HeaderMap::new();
            for (name, value) in &endpoint.headers() {
                let (name, value) = parse_header(&format!("{name}: {value}"))
                    .map_err(PepeError::HeaderParseError)?;
                headers.append(name, value);
            }
            let urls = self.urls(index);
            for url in &urls {
                targets.push(Target {
                    request: Request::new(
                        url.clone(),
                        endpoint.method.clone(),
                        endpoint.body.clone(),
                        &[],
                        cli.settings(),
                    )?,
                    headers: headers.clone(),
                    endpoint: tag as u16,
                    // The endpoint's share is split between its URLs, so
                    // rotating through values doesn't multiply its traffic
                    weight: (endpoint.weight * VARIANT_SCALE / urls.len() as u32).max(1),
                });
            }
        }
        Ok(targets)
    }

    /// What the dashboard shows for each of these endpoints. Credentials
    /// are named, never shown.
    pub fn views(&self, cli: &Cli, which: &[usize]) -> Vec<EndpointView> {
        let masked = self.credentials.masked();
        let mut shared: Vec<(String, String)> = cli
            .headers
            .iter()
            .filter_map(|h| h.split_once(':'))
            .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
            .collect();
        shared.extend(masked.headers.iter().cloned());
        which
            .iter()
            .map(|&index| {
                let endpoint = &self.endpoints[index];
                let mut headers = shared.clone();
                headers.extend(endpoint.headers());
                let urls = endpoint.urls(&self.spec.base_url, &masked);
                EndpointView {
                    label: endpoint.label.clone(),
                    method: endpoint.method.clone(),
                    variants: urls.len(),
                    url: urls.into_iter().next().unwrap_or_default(),
                    headers,
                    body: endpoint.body.clone(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn an_endpoints_share_is_split_across_its_urls() {
        let doc: serde_json::Value = serde_json::json!({
            "openapi": "3.0.0", "servers": [{"url": "https://api.demo.io"}],
            "paths": {
                "/a": {"get": {}},
                "/b/{id}": {"get": {"parameters": [{"name": "id", "in": "path", "required": true}]}}
            }
        });
        let spec = Spec::parse(&doc, None, None).unwrap();
        let options = PlanOptions {
            set: vec![(
                "id".into(),
                vec!["1".into(), "2".into(), "3".into(), "4".into()],
            )],
            all: true,
            ..Default::default()
        };
        let credentials = Credentials::parse(&["bearer:t".into()], &spec).unwrap();
        let endpoints = openapi::plan(&spec, &options);
        let run = ApiRun {
            spec,
            options,
            credentials,
            endpoints,
        };
        let cli = Cli::parse_from(["pepe", "-H", "X-Run: 1", "api", "x"]);

        let targets = run.targets(&cli, &run.enabled()).unwrap();
        assert_eq!(targets.len(), 5, "one for /a, four for /b/{{id}}");
        let share = |endpoint| -> u32 {
            targets
                .iter()
                .filter(|t| t.endpoint == endpoint)
                .map(|t| t.weight)
                .sum()
        };
        assert_eq!(share(0), share(1), "both endpoints get the same traffic");

        // -H flags and credentials reach every endpoint's view
        let views = run.views(&cli, &run.enabled());
        assert_eq!(views[1].variants, 4);
        assert!(views[0].headers.contains(&("X-Run".into(), "1".into())));
        assert!(
            views[0]
                .headers
                .contains(&("Authorization".into(), openapi::MASK.into())),
            "credentials are named, not shown"
        );
    }
}
