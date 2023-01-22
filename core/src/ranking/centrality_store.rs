// Cuely is an open source web search engine.
// Copyright (C) 2022 Cuely ApS
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

use std::io::{BufReader, BufWriter, Read};
use std::{collections::BTreeMap, fs::File, path::Path};

use crate::Result;
use crate::{
    kv::{rocksdb_store::RocksDbStore, Kv},
    webgraph::{
        centrality::{harmonic::HarmonicCentrality, online_harmonic::OnlineHarmonicCentrality},
        Node, NodeID, Webgraph,
    },
};

use super::inbound_similarity::InboundSimilarity;

pub struct HarmonicCentralityStore {
    pub host: Box<dyn Kv<String, f64>>,
    pub full: Box<dyn Kv<String, f64>>,
}

impl HarmonicCentralityStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Self {
        Self {
            host: RocksDbStore::open(path.as_ref().join("host")),
            full: RocksDbStore::open(path.as_ref().join("full")),
        }
    }

    fn flush(&self) {
        self.host.flush();
        self.full.flush();
    }
}

pub struct IndexerCentralityStore {
    pub harmonic: HarmonicCentralityStore,
    pub node2id: BTreeMap<Node, NodeID>,
}

impl IndexerCentralityStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Self {
        Self {
            harmonic: HarmonicCentralityStore::open(path.as_ref().join("harmonic")),
            node2id: open_node2id(path.as_ref().join("node2id")).unwrap(),
        }
    }
}

impl From<CentralityStore> for IndexerCentralityStore {
    fn from(store: CentralityStore) -> Self {
        Self {
            node2id: store.node2id,
            harmonic: store.harmonic,
        }
    }
}

pub struct SearchCentralityStore {
    pub online_harmonic: OnlineHarmonicCentrality,
    pub inbound_similarity: InboundSimilarity,
    pub node2id: BTreeMap<Node, NodeID>,
}

impl SearchCentralityStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Self {
        Self {
            online_harmonic: OnlineHarmonicCentrality::open(path.as_ref().join("online_harmonic"))
                .unwrap(),
            inbound_similarity: InboundSimilarity::open(path.as_ref().join("inbound_similarity"))
                .unwrap(),
            node2id: open_node2id(path.as_ref().join("node2id")).unwrap(),
        }
    }
}

impl From<CentralityStore> for SearchCentralityStore {
    fn from(store: CentralityStore) -> Self {
        Self {
            online_harmonic: store.online_harmonic,
            inbound_similarity: store.inbound_similarity,
            node2id: store.node2id,
        }
    }
}

pub struct CentralityStore {
    pub harmonic: HarmonicCentralityStore,
    pub online_harmonic: OnlineHarmonicCentrality,
    pub inbound_similarity: InboundSimilarity,
    pub node2id: BTreeMap<Node, NodeID>,
    pub base_path: String,
}

fn open_node2id<P: AsRef<Path>>(path: P) -> Result<BTreeMap<Node, NodeID>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;

    Ok(bincode::deserialize(&buf)?)
}

impl CentralityStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Self {
        Self {
            harmonic: HarmonicCentralityStore::open(path.as_ref().join("harmonic")),
            online_harmonic: OnlineHarmonicCentrality::open(path.as_ref().join("online_harmonic"))
                .ok()
                .unwrap_or_default(),
            inbound_similarity: InboundSimilarity::open(path.as_ref().join("inbound_similarity"))
                .ok()
                .unwrap_or_default(),
            node2id: open_node2id(path.as_ref().join("node2id"))
                .ok()
                .unwrap_or_default(),
            base_path: path.as_ref().to_str().unwrap().to_string(),
        }
    }

    fn store_host<P: AsRef<Path>>(
        output_path: P,
        store: &mut CentralityStore,
        harmonic_centrality: &HarmonicCentrality,
    ) {
        let csv_file = File::options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(output_path.as_ref().join("harmonic_host.csv"))
            .unwrap();

        let mut host: Vec<_> = harmonic_centrality.host.clone().into_iter().collect();
        host.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut wtr = csv::Writer::from_writer(csv_file);
        for (node, centrality) in host {
            store.harmonic.host.insert(node.name.clone(), centrality);
            wtr.write_record(&[node.name, centrality.to_string()])
                .unwrap();
        }
        wtr.flush().unwrap();
    }

    pub fn build<P: AsRef<Path>>(graph: &Webgraph, output_path: P) -> Self {
        let mut store = CentralityStore::open(output_path.as_ref());

        let harmonic_centrality = HarmonicCentrality::calculate(graph);

        Self::store_host(&output_path, &mut store, &harmonic_centrality);
        store.online_harmonic = OnlineHarmonicCentrality::new(graph, &harmonic_centrality);
        store.inbound_similarity = InboundSimilarity::build(graph, &harmonic_centrality);

        store.node2id = graph.node_ids().collect();

        store.flush();

        store
    }

    pub fn flush(&self) {
        self.harmonic.flush();

        self.online_harmonic
            .save(Path::new(&self.base_path).join("online_harmonic"))
            .unwrap();

        self.inbound_similarity
            .save(Path::new(&self.base_path).join("inbound_similarity"))
            .unwrap();

        let mut file = BufWriter::new(
            File::options()
                .create(true)
                .truncate(true)
                .write(true)
                .open(Path::new(&self.base_path).join("node2id"))
                .unwrap(),
        );
        bincode::serialize_into(&mut file, &self.node2id).unwrap();
    }
}
