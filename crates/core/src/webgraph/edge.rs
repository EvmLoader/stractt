// Stract is an open source web search engine.
// Copyright (C) 2024 Stract ApS
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
// along with this program.  If not, see <https://www.gnu.org/licenses/

use utoipa::ToSchema;

use super::{FullNodeID, Node, NodeID};

pub const MAX_LABEL_LENGTH: usize = 1024;

pub trait EdgeLabel
where
    Self: Send + Sync + Sized,
{
    fn to_bytes(&self) -> anyhow::Result<Vec<u8>>;
    fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self>;
}

impl EdgeLabel for String {
    fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        Ok(self.as_bytes().to_vec())
    }

    fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(String::from_utf8(bytes.to_vec())?)
    }
}

impl EdgeLabel for () {
    fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        Ok(Vec::new())
    }

    fn from_bytes(_bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Edge<L>
where
    L: EdgeLabel,
{
    pub from: NodeID,
    pub to: NodeID,
    pub label: L,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InnerEdge<L>
where
    L: EdgeLabel,
{
    pub from: FullNodeID,
    pub to: FullNodeID,
    pub label: L,
}

impl<L> From<InnerEdge<L>> for Edge<L>
where
    L: EdgeLabel,
{
    fn from(edge: InnerEdge<L>) -> Self {
        Edge {
            from: edge.from.id,
            to: edge.to.id,
            label: edge.label,
        }
    }
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash, ToSchema, tapi::Tapi,
)]
#[serde(rename_all = "camelCase")]
pub struct FullEdge {
    pub from: Node,
    pub to: Node,
    pub label: String,
}
