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

use crate::config::defaults;
use http::StatusCode;
use optics::{HostRankings, Optic};
use utoipa::ToSchema;

use axum::Json;
use axum_macros::debug_handler;

use crate::{
    bangs::BangHit,
    searcher::{self, SearchQuery, SearchResult, WebsitesResult},
    webpage::region::Region,
};

use super::AppState;

use axum::{extract, response::IntoResponse};

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema, tapi::Tapi)]
#[serde(rename_all = "camelCase")]
#[schema(title = "SearchQuery", example = json!({"query": "hello world"}))]
pub struct ApiSearchQuery {
    pub query: String,
    pub page: Option<usize>,
    pub num_results: Option<usize>,
    pub selected_region: Option<Region>,
    pub optic: Option<String>,
    pub host_rankings: Option<HostRankings>,
    pub safe_search: Option<bool>,

    #[serde(default = "defaults::SearchQuery::return_ranking_signals")]
    pub return_ranking_signals: bool,

    #[serde(default = "defaults::SearchQuery::flatten_response")]
    pub flatten_response: bool,

    #[serde(default = "defaults::SearchQuery::count_results")]
    pub count_results: bool,
}

impl TryFrom<ApiSearchQuery> for SearchQuery {
    type Error = anyhow::Error;

    fn try_from(api: ApiSearchQuery) -> Result<Self, Self::Error> {
        let optic = if let Some(optic) = &api.optic {
            Some(Optic::parse(optic)?)
        } else {
            None
        };

        let default = SearchQuery::default();

        Ok(SearchQuery {
            query: api.query,
            page: api.page.unwrap_or(default.page),
            num_results: api.num_results.unwrap_or(default.num_results),
            selected_region: api.selected_region,
            optic,
            host_rankings: api.host_rankings,
            return_ranking_signals: api.return_ranking_signals,
            safe_search: api.safe_search.unwrap_or(default.safe_search),
            count_results: api.count_results,
        })
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema, tapi::Tapi)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ApiSearchResult {
    Websites(WebsitesResult),
    Bang(Box<BangHit>),
}

impl From<SearchResult> for ApiSearchResult {
    fn from(result: SearchResult) -> Self {
        match result {
            SearchResult::Websites(result) => ApiSearchResult::Websites(result),
            SearchResult::Bang(result) => ApiSearchResult::Bang(result),
        }
    }
}

#[debug_handler]
#[utoipa::path(
    post,
    path = "/beta/api/search",
    request_body(content = ApiSearchQuery),
    responses(
        (status = 200, description = "Search results", body = ApiSearchResult),
    )
)]
#[tapi::tapi(path = "/search", method = Post, state = AppState)]
pub async fn search(
    extract::State(state): extract::State<AppState>,
    extract::Json(query): extract::Json<ApiSearchQuery>,
) -> tapi::endpoints::OneOf5<
    extract::Json<ApiSearchResult>,
    extract::Json<SearchResult>,
    String,
    tapi::endpoints::Statused<400, ()>,
    tapi::endpoints::Statused<500, ()>,
> {
    tracing::debug!(?query);
    let flatten_result = query.flatten_response;
    let query = SearchQuery::try_from(query);

    if let Err(err) = query {
        tracing::error!("{:?}", err);
        return tapi::endpoints::OneOf5::D(().into());
    }
    let mut query = query.unwrap();

    query.num_results = query.num_results.min(100);

    match state.searcher.search(&query).await {
        Ok(result) => {
            if flatten_result {
                tapi::endpoints::OneOf5::A(Json(ApiSearchResult::from(result)))
            } else {
                tapi::endpoints::OneOf5::B(Json(result))
            }
        }

        Err(err) => match err.downcast_ref() {
            Some(searcher::distributed::Error::EmptyQuery) => {
                tapi::endpoints::OneOf5::C(searcher::distributed::Error::EmptyQuery.to_string())
            }
            _ => {
                tracing::error!("{:?}", err);
                tapi::endpoints::OneOf5::E(().into())
            }
        },
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema, tapi::Tapi)]
pub struct WidgetQuery {
    pub query: String,
}

#[debug_handler]
#[utoipa::path(
    post,
    path = "/beta/api/search/widget",
    request_body(content = WidgetQuery),
    responses(
        (status = 200, description = "The resulting widget if one matches the query", body = Option<Widget>),
    )
)]
#[tapi::tapi(path = "/search/widget", method = Post, state = AppState)]
pub async fn widget(
    extract::State(state): extract::State<AppState>,
    extract::Json(req): extract::Json<WidgetQuery>,
) -> extract::Json<Option<crate::widgets::Widget>> {
    Json(state.searcher.widget(&req.query).await)
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema, tapi::Tapi)]
pub struct SidebarQuery {
    pub query: String,
}

#[debug_handler]
#[utoipa::path(
    post,
    path = "/beta/api/search/sidebar",
    request_body(content = SidebarQuery),
    responses(
        (status = 200, description = "The sidebar if one matches the query", body = Option<DisplayedSidebar>),
    )
)]
#[tapi::tapi(path = "/search/sidebar", method = Post, state = AppState)]
pub async fn sidebar(
    extract::State(state): extract::State<AppState>,
    extract::Json(req): extract::Json<SidebarQuery>,
) -> Json<Option<crate::search_prettifier::DisplayedSidebar>> {
    Json(state.searcher.sidebar(&req.query).await)
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema, tapi::Tapi)]
pub struct SpellcheckQuery {
    pub query: String,
}

#[debug_handler]
#[utoipa::path(
    post,
    path = "/beta/api/search/spellcheck",
    request_body(content = SpellcheckQuery),
    responses(
        (status = 200, description = "The corrected string with the changes highlighted using <b>...<\\b> elements. Returns empty response if there is no correction to be made.", body = Option<HighlightedSpellCorrection>),
    )
)]
#[tapi::tapi(path = "/search/spellcheck", method = Post, state = AppState)]
pub async fn spellcheck(
    extract::State(state): extract::State<AppState>,
    extract::Json(req): extract::Json<SpellcheckQuery>,
) -> Json<Option<crate::search_prettifier::HighlightedSpellCorrection>> {
    Json(state.searcher.spell_check(&req.query))
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema, tapi::Tapi)]
#[serde(rename_all = "camelCase")]
pub struct EntityImageParams {
    pub image_id: String,
    pub max_width: Option<u64>,
    pub max_height: Option<u64>,
}

#[utoipa::path(
    post,
    path = "/beta/api/entity_image",
    request_body(content = ApiSearchQuery),
    responses(
        (status = 200, description = "Search results", body = ApiSearchResult),
    )
)]
// TODO
// #[tapi::tapi(path = "/entity_image", method = Post, state = AppState)]
pub async fn entity_image(
    extract::Query(query): extract::Query<EntityImageParams>,
    extract::State(state): extract::State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    match state
        .searcher
        .get_entity_image(&query.image_id, query.max_height, query.max_width)
        .await
    {
        Ok(Some(result)) => {
            let bytes = result.as_raw_bytes();

            Ok((
                ([(axum::http::header::CONTENT_TYPE, "image/png")]),
                axum::response::AppendHeaders([(axum::http::header::CONTENT_TYPE, "image/png")]),
                bytes,
            ))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(err) => {
            tracing::error!("{:?}", err);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
