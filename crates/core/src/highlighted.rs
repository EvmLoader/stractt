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
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use utoipa::ToSchema;

#[derive(
    Default,
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    bincode::Encode,
    bincode::Decode,
    PartialEq,
    ToSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct Highlighted {
    pub text: String,
    pub fragments: Vec<HighlightedFragment<(usize, usize)>>,
}

impl Highlighted {
    pub fn push(&mut self, fragment: HighlightedFragment<String>) {
        let start = self.text.len();
        self.text.push_str(fragment.text());
        let end = self.text.len();
        self.fragments.push(HighlightedFragment {
            kind: fragment.kind,
            text: (start, end),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = HighlightedFragment<&str>> {
        self.fragments.iter().map(|f| HighlightedFragment {
            kind: f.kind,
            text: &self.text[f.text.0..f.text.1],
        })
    }
}

impl<const N: usize> From<[HighlightedFragment; N]> for Highlighted {
    fn from(value: [HighlightedFragment; N]) -> Self {
        let mut acc = Highlighted::default();

        for frag in value {
            acc.push(frag);
        }

        acc
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    serde::Serialize,
    serde::Deserialize,
    bincode::Encode,
    bincode::Decode,
    PartialEq,
    ToSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum HighlightedKind {
    Normal,
    Highlighted,
}

#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    bincode::Encode,
    bincode::Decode,
    PartialEq,
    ToSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct HighlightedFragment<T = String> {
    pub kind: HighlightedKind,
    pub text: T,
}

impl<T> HighlightedFragment<T> {
    pub fn new_unhighlighted(text: T) -> Self {
        Self::new_normal(text)
    }

    pub fn new_normal(text: T) -> Self {
        Self {
            kind: HighlightedKind::Normal,
            text,
        }
    }

    pub fn new_highlighted(text: T) -> Self {
        Self {
            kind: HighlightedKind::Highlighted,
            text,
        }
    }
}

impl<T: std::ops::Deref<Target = str>> HighlightedFragment<T> {
    pub fn text(&self) -> &str {
        &self.text
    }
}
