// Stract is an open source web search engine.
// Copyright (C) 2023 Stract ApS
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract, Json};
use utoipa::{IntoParams, ToSchema};

use crate::{
    config::WebgraphGranularity,
    distributed::{cluster::Cluster, member::Service, retry_strategy::ExponentialBackoff, sonic},
    webgraph::{FullEdge, Node},
};

use super::AppState;

pub struct RemoteWebgraph {
    cluster: Arc<Cluster>,
}

impl RemoteWebgraph {
    pub fn new(cluster: Arc<Cluster>) -> Self {
        Self { cluster }
    }

    async fn host(&self, level: WebgraphGranularity) -> Option<SocketAddr> {
        self.cluster
            .members()
            .await
            .iter()
            .find_map(|member| match member.service {
                Service::Webgraph { host, granularity } if granularity == level => Some(host),
                _ => None,
            })
    }
}

pub mod host {
    use url::Url;

    use crate::entrypoint::webgraph_server::ScoredHost;

    use super::*;

    #[derive(serde::Deserialize, ToSchema, tapi::Tapi)]
    #[serde(rename_all = "camelCase")]
    pub struct SimilarHostsParams {
        pub hosts: Vec<String>,
        pub top_n: usize,
    }

    #[derive(serde::Deserialize, IntoParams, tapi::Tapi)]
    #[serde(rename_all = "camelCase")]
    pub struct KnowsHostParams {
        pub host: String,
    }

    #[derive(serde::Deserialize, IntoParams, tapi::Tapi)]
    #[serde(rename_all = "camelCase")]
    pub struct HostLinksParams {
        pub host: String,
    }

    #[utoipa::path(post,
        path = "/beta/api/webgraph/host/similar",
        request_body(content = SimilarHostsParams),
        responses(
            (status = 200, description = "List of similar hosts", body = Vec<ScoredHost>),
        )
    )]
    #[tapi::tapi(path = "/host/similar", method = Post, state = AppState)]
    pub async fn similar(
        extract::State(state): extract::State<AppState>,
        extract::Json(params): extract::Json<SimilarHostsParams>,
    ) -> tapi::endpoints::OneOf2<Json<Vec<ScoredHost>>, tapi::endpoints::Statused<500, ()>> {
        state.counters.explore_counter.inc();
        let Some(host) = state.remote_webgraph.host(WebgraphGranularity::Host).await else {
            return tapi::endpoints::OneOf2::B(().into());
        };

        let retry = ExponentialBackoff::from_millis(30)
            .with_limit(Duration::from_millis(200))
            .take(5);

        let Ok(conn) = sonic::service::Connection::create_with_timeout_retry(
            host,
            Duration::from_secs(30),
            retry,
        )
        .await
        else {
            return tapi::endpoints::OneOf2::B(().into());
        };

        match conn
            .send_with_timeout(
                &crate::entrypoint::webgraph_server::SimilarHosts {
                    hosts: params.hosts,
                    top_n: params.top_n,
                },
                Duration::from_secs(60),
            )
            .await
        {
            Ok(nodes) => tapi::endpoints::OneOf2::A(Json(nodes)),
            Err(err) => {
                tracing::error!("Failed to send request to webgraph: {}", err);
                tapi::endpoints::OneOf2::B(().into())
            }
        }
    }

    #[utoipa::path(post,
        path = "/beta/api/webgraph/host/knows",
        params(KnowsHostParams),
        responses(
            (status = 200, description = "Whether the host is known", body = KnowsHost),
        )
    )]
    #[tapi::tapi(path = "/host/knows", method = Post, state = AppState)]
    pub async fn knows(
        extract::State(state): extract::State<AppState>,
        extract::Query(params): extract::Query<KnowsHostParams>,
    ) -> tapi::endpoints::OneOf2<Json<KnowsHost>, tapi::endpoints::Statused<500, ()>> {
        let Some(host) = state.remote_webgraph.host(WebgraphGranularity::Host).await else {
            return tapi::endpoints::OneOf2::B(().into());
        };

        let retry = ExponentialBackoff::from_millis(30)
            .with_limit(Duration::from_millis(200))
            .take(5);

        let Ok(conn) = sonic::service::Connection::create_with_timeout_retry(
            host,
            Duration::from_secs(30),
            retry,
        )
        .await
        else {
            return tapi::endpoints::OneOf2::B(().into());
        };

        let response = match conn
            .send_with_timeout(
                &crate::entrypoint::webgraph_server::Knows { host: params.host },
                Duration::from_secs(60),
            )
            .await
        {
            Ok(Some(node)) => Json(KnowsHost::Known {
                host: node.as_str().to_string(),
            }),
            Err(err) => {
                tracing::error!("Failed to send request to webgraph: {}", err);
                Json(KnowsHost::Unknown)
            }
            _ => Json(KnowsHost::Unknown),
        };
        tapi::endpoints::OneOf2::A(response)
    }

    #[utoipa::path(post,
        path = "/beta/api/webgraph/host/ingoing",
        params(HostLinksParams),
        responses(
            (status = 200, description = "Incoming links for a particular host", body = Vec<FullEdge>),
        )
    )]
    #[tapi::tapi(path = "/host/ingoing", method = Post, state = AppState)]
    pub async fn ingoing_hosts(
        extract::State(state): extract::State<AppState>,
        extract::Query(params): extract::Query<HostLinksParams>,
    ) -> tapi::endpoints::OneOf2<Json<Vec<FullEdge>>, tapi::endpoints::Statused<500, ()>> {
        let Ok(url) = Url::parse(&("http://".to_string() + params.host.as_str())) else {
            return tapi::endpoints::OneOf2::B(().into());
        };
        let node = Node::from(url).into_host();
        let Ok(links) = ingoing_links(state, node, WebgraphGranularity::Host).await else {
            tracing::error!("Failed to send request to webgraph");
            return tapi::endpoints::OneOf2::B(().into());
        };

        tapi::endpoints::OneOf2::A(Json(links))
    }

    #[utoipa::path(post,
        path = "/beta/api/webgraph/host/outgoing",
        params(HostLinksParams),
        responses(
            (status = 200, description = "Outgoing links for a particular host", body = Vec<FullEdge>),
        )
    )]
    #[tapi::tapi(path = "/host/outgoing", method = Post, state = AppState)]
    pub async fn outgoing_hosts(
        extract::State(state): extract::State<AppState>,
        extract::Query(params): extract::Query<HostLinksParams>,
    ) -> tapi::endpoints::OneOf2<Json<Vec<FullEdge>>, tapi::endpoints::Statused<500, ()>> {
        let Ok(url) = Url::parse(&("http://".to_string() + params.host.as_str())) else {
            return tapi::endpoints::OneOf2::B(().into());
        };
        let node = Node::from(url).into_host();
        let Ok(links) = outgoing_links(state, node, WebgraphGranularity::Host).await else {
            tracing::error!("Failed to send request to webgraph");
            return tapi::endpoints::OneOf2::B(().into());
        };

        tapi::endpoints::OneOf2::A(Json(links))
    }
}

pub mod page {
    use super::*;

    #[derive(serde::Deserialize, IntoParams, tapi::Tapi)]
    #[serde(rename_all = "camelCase")]
    pub struct PageLinksParams {
        pub page: String,
    }

    #[utoipa::path(post,
        path = "/beta/api/webgraph/page/ingoing",
        params(PageLinksParams),
        responses(
            (status = 200, description = "Incoming links for a particular page", body = Vec<FullEdge>),
        )
    )]
    #[tapi::tapi(path = "/page/ingoing", method = Post, state = AppState)]
    pub async fn ingoing_pages(
        extract::State(state): extract::State<AppState>,
        extract::Query(params): extract::Query<PageLinksParams>,
    ) -> tapi::endpoints::OneOf2<Json<Vec<FullEdge>>, tapi::endpoints::Statused<500, ()>> {
        let node = Node::from(params.page);
        let Ok(links) = ingoing_links(state, node, WebgraphGranularity::Page).await else {
            tracing::error!("Failed to send request to webgraph");
            return tapi::endpoints::OneOf2::B(().into());
        };

        tapi::endpoints::OneOf2::A(Json(links))
    }

    #[utoipa::path(post,
        path = "/beta/api/webgraph/page/outgoing",
        params(PageLinksParams),
        responses(
            (status = 200, description = "Outgoing links for a particular page", body = Vec<FullEdge>),
        )
    )]
    #[tapi::tapi(path = "/page/outgoing", method = Post, state = AppState)]
    pub async fn outgoing_pages(
        extract::State(state): extract::State<AppState>,
        extract::Query(params): extract::Query<PageLinksParams>,
    ) -> tapi::endpoints::OneOf2<Json<Vec<FullEdge>>, tapi::endpoints::Statused<500, ()>> {
        let node = Node::from(params.page);
        let Ok(links) = outgoing_links(state, node, WebgraphGranularity::Page).await else {
            tracing::error!("Failed to send request to webgraph");
            return tapi::endpoints::OneOf2::B(().into());
        };

        tapi::endpoints::OneOf2::A(Json(links))
    }
}

async fn ingoing_links(
    state: AppState,
    node: Node,
    level: WebgraphGranularity,
) -> anyhow::Result<Vec<FullEdge>> {
    let host = state
        .remote_webgraph
        .host(level)
        .await
        .ok_or(anyhow::anyhow!(
            "no remote webgraph for granularity {level:?}"
        ))?;

    let retry = ExponentialBackoff::from_millis(30)
        .with_limit(Duration::from_millis(200))
        .take(5);

    let conn =
        sonic::service::Connection::create_with_timeout_retry(host, Duration::from_secs(30), retry)
            .await?;

    Ok(conn
        .send_with_timeout(
            &crate::entrypoint::webgraph_server::IngoingLinks { node },
            Duration::from_secs(60),
        )
        .await?)
}

async fn outgoing_links(
    state: AppState,
    node: Node,
    level: WebgraphGranularity,
) -> anyhow::Result<Vec<FullEdge>> {
    let host = state
        .remote_webgraph
        .host(level)
        .await
        .ok_or(anyhow::anyhow!(
            "no remote webgraph for granularity {level:?}"
        ))?;

    let retry = ExponentialBackoff::from_millis(30)
        .with_limit(Duration::from_millis(200))
        .take(5);

    let conn =
        sonic::service::Connection::create_with_timeout_retry(host, Duration::from_secs(30), retry)
            .await?;

    Ok(conn
        .send_with_timeout(
            &crate::entrypoint::webgraph_server::OutgoingLinks { node },
            Duration::from_secs(60),
        )
        .await?)
}

#[derive(serde::Serialize, serde::Deserialize, ToSchema, tapi::Tapi)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum KnowsHost {
    Known { host: String },
    Unknown,
}
