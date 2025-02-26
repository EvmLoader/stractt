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
use futures::StreamExt;
use std::net::SocketAddr;
use std::path::Path;

use itertools::Itertools;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::pin;
use tracing::{debug, info, trace};

use crate::entrypoint::async_download_all_warc_files;
use crate::index::{FrozenIndex, Index};
use crate::mapreduce::{Manager, Map, Reduce, Worker};
use crate::ranking::centrality_store::CentralityStore;
use crate::ranking::SignalAggregator;
use crate::warc::WarcFile;
use crate::webgraph::{Node, Webgraph, WebgraphBuilder};
use crate::webpage::{Html, Link, Webpage};
use crate::{
    HttpConfig, IndexingLocalConfig, IndexingMasterConfig, LocalConfig, Result, WarcSource,
};

pub struct Indexer {}

#[derive(Debug, Serialize, Deserialize, Clone)]
enum JobConfig {
    Http(HttpConfig),
    Local(LocalConfig),
}

#[derive(Debug, Serialize, Deserialize)]
struct Job {
    source_config: JobConfig,
    download_images: bool,
    warc_paths: Vec<String>,
    base_path: String,
    host_centrality_threshold: Option<f64>,
}

struct IndexingWorker {
    host_centrality_store: CentralityStore,
    page_centrality_store: CentralityStore,
    webgraph: Option<Webgraph>,
}

impl IndexingWorker {
    fn new(centrality_store_path: String, webgraph_path: Option<String>) -> Self {
        let host_centrality_path = Path::new(&centrality_store_path).join("host");
        let page_centrality_path = Path::new(&centrality_store_path).join("full");

        Self {
            host_centrality_store: CentralityStore::new(host_centrality_path),
            page_centrality_store: CentralityStore::new(page_centrality_path),
            webgraph: webgraph_path.map(|path| {
                WebgraphBuilder::new(path)
                    .with_full_graph()
                    .with_host_graph()
                    .read_only(true)
                    .open()
            }),
        }
    }
}

async fn async_process_job(job: &Job, worker: &IndexingWorker) -> Index {
    let name = job.warc_paths.first().unwrap().split('/').last().unwrap();

    info!("processing {}", name);

    let mut index = Index::open(Path::new(&job.base_path).join(name)).unwrap();

    let source = match job.source_config.clone() {
        JobConfig::Http(config) => WarcSource::HTTP(config),
        JobConfig::Local(config) => WarcSource::Local(config),
    };

    let warc_files = async_download_all_warc_files(&job.warc_paths, &source, &job.base_path).await;
    pin!(warc_files);

    let signal_aggregator = SignalAggregator::default();

    while let Some(file) = warc_files.next().await {
        let name = file.split('/').last().unwrap();
        let path = Path::new(&job.base_path).join("warc_files").join(name);

        if let Ok(file) = WarcFile::open(path) {
            for record in
                file.records()
                    .flatten()
                    .filter(|record| match &record.response.payload_type {
                        Some(payload_type) => !matches!(payload_type.as_str(), "application/pdf"),
                        None => true,
                    })
            {
                let mut html = Html::parse_without_text(&record.response.body, &record.request.url);

                let host_centrality = worker
                    .host_centrality_store
                    .get(html.url().host_without_specific_subdomains())
                    .unwrap_or_default();

                if let Some(host_centrality_threshold) = job.host_centrality_threshold {
                    if host_centrality < host_centrality_threshold {
                        continue;
                    }
                }

                html.parse_text();

                let backlinks: Vec<Link> = worker
                    .webgraph
                    .as_ref()
                    .map(|webgraph| {
                        webgraph
                            .ingoing_edges(Node::from(html.url()))
                            .into_iter()
                            .map(|edge| Link {
                                source: edge.from.name.into(),
                                destination: edge.to.name.into(),
                                text: edge.label,
                            })
                            .collect()
                    })
                    .unwrap_or_else(Vec::new);

                let page_centrality = worker
                    .page_centrality_store
                    .get(html.url().raw())
                    .unwrap_or_default();

                let fetch_time_ms = record.metadata.fetch_time_ms as u64;

                trace!("inserting webpage: {:?}", html.url());

                trace!("title = {:?}", html.title());
                trace!("text = {:?}", html.clean_text());

                let mut webpage = Webpage {
                    html,
                    backlinks,
                    page_centrality,
                    host_centrality,
                    fetch_time_ms,
                    primary_image: None,
                    pre_computed_score: 0.0,
                };

                webpage.pre_computed_score =
                    signal_aggregator.precompute_score(&webpage, &index.region_count);

                if let Err(err) = index.insert(webpage) {
                    debug!("{:?}", err);
                }
            }
            if job.download_images {
                info!("downloading images");
                index.download_pending_images();
            }
        }

        index.commit().unwrap();

        std::fs::remove_file(file).ok();
    }

    info!("{} done", name);

    index
}

fn process_job(job: &Job, worker: &IndexingWorker) -> Index {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async { async_process_job(job, worker).await })
}

#[derive(Debug, Serialize, Deserialize)]
struct IndexPointer(String);

impl Worker for IndexingWorker {}

impl Map<IndexingWorker, FrozenIndex> for Job {
    fn map(&self, worker: &IndexingWorker) -> FrozenIndex {
        let index = process_job(self, worker);
        index.into()
    }
}

impl Map<IndexingWorker, IndexPointer> for Job {
    fn map(&self, worker: &IndexingWorker) -> IndexPointer {
        let index = process_job(self, worker);
        IndexPointer(index.path)
    }
}

impl Reduce<FrozenIndex> for Index {
    fn reduce(self, element: FrozenIndex) -> Self {
        let other: Index = element.into();

        let other_path = other.path.clone();

        let res = self.merge(other);

        std::fs::remove_dir_all(other_path).unwrap();

        res
    }
}

impl Reduce<IndexPointer> for IndexPointer {
    fn reduce(self, element: IndexPointer) -> Self {
        let index = Index::open(self.0).unwrap();
        let other_path = element.0;
        let other = Index::open(&other_path).unwrap();

        let res = index.merge(other);

        std::fs::remove_dir_all(other_path).unwrap();

        IndexPointer(res.path)
    }
}

impl Reduce<Index> for Index {
    fn reduce(self, element: Index) -> Self {
        let other = element;
        let other_path = other.path.clone();

        let res = self.merge(other);

        std::fs::remove_dir_all(other_path).unwrap();

        res
    }
}

impl Indexer {
    pub fn run_master(config: &IndexingMasterConfig) -> Result<()> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                info!("Running master for index construction");

                let warc_paths = config.warc_source.paths().unwrap();

                let workers: Vec<SocketAddr> = config
                    .workers
                    .iter()
                    .map(|worker| worker.parse().unwrap())
                    .collect();

                let job_config = match config.warc_source.clone() {
                    WarcSource::HTTP(config) => JobConfig::Http(config),
                    WarcSource::Local(config) => JobConfig::Local(config),
                };

                let mut warc_paths: Box<dyn Iterator<Item = Job> + Send> = Box::new(
                    warc_paths
                        .into_iter()
                        .chunks(config.batch_size.unwrap_or(1))
                        .into_iter()
                        .map(|warc_paths| Job {
                            source_config: job_config.clone(),
                            warc_paths: warc_paths.collect_vec(),
                            download_images: config.download_images.unwrap_or(true),
                            host_centrality_threshold: config.host_centrality_threshold,
                            base_path: config
                                .index_base_path
                                .clone()
                                .unwrap_or_else(|| "data/index".to_string()),
                        })
                        .collect_vec()
                        .into_iter(),
                );

                if let Some(limit) = config.limit_warc_files {
                    warc_paths = Box::new(warc_paths.take(limit));
                }

                let manager = Manager::new(&workers);
                let mut index: Index = manager
                    .run::<IndexingWorker, Job, FrozenIndex, Index>(warc_paths)
                    .await
                    .unwrap();

                index
                    .inverted_index
                    .merge_into_segments(config.final_num_segments.unwrap_or(20))
                    .unwrap();
            });

        Ok(())
    }

    pub fn run_worker(
        worker_addr: String,
        centrality_store_path: String,
        webgraph_path: Option<String>,
    ) -> Result<()> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                IndexingWorker::new(centrality_store_path, webgraph_path)
                    .run::<Job, FrozenIndex>(
                        worker_addr
                            .parse::<SocketAddr>()
                            .expect("Could not parse worker address"),
                    )
                    .await
                    .unwrap();
            });
        Ok(())
    }

    pub fn run_locally(config: &IndexingLocalConfig) -> Result<()> {
        let warc_paths = config.warc_source.paths()?;

        let job_config = match config.warc_source.clone() {
            WarcSource::HTTP(config) => JobConfig::Http(config),
            WarcSource::Local(config) => JobConfig::Local(config),
        };

        let worker = IndexingWorker::new(
            config.centrality_store_path.clone(),
            config.webgraph_path.clone(),
        );

        let index = warc_paths
            .into_iter()
            .take(config.limit_warc_files.unwrap_or(usize::MAX))
            .chunks(config.batch_size.unwrap_or(1))
            .into_iter()
            .map(|warc_paths| Job {
                source_config: job_config.clone(),
                warc_paths: warc_paths.collect_vec(),
                download_images: config.download_images.unwrap_or(true),
                host_centrality_threshold: config.host_centrality_threshold,
                base_path: config
                    .output_path
                    .clone()
                    .unwrap_or_else(|| "data/index".to_string()),
            })
            .collect_vec()
            // .into_iter()
            // .map(|job| job.map(&worker))
            // .fold(None, |acc: Option<Index>, elem: FrozenIndex| match acc {
            //     Some(acc) => Some(acc.reduce(elem)),
            //     None => Some(elem.into()),
            // });
            .into_par_iter()
            .panic_fuse()
            .map(|job| -> IndexPointer { job.map(&worker) })
            .map(Some)
            .reduce(
                || None,
                |a, b| match (a, b) {
                    (Some(a), Some(b)) => Some(a.reduce(b)),
                    (Some(graph), None) | (None, Some(graph)) => Some(graph),
                    (None, None) => None,
                },
            );

        if let Some(pointer) = index {
            let mut index = Index::open(pointer.0).unwrap();

            index
                .inverted_index
                .merge_into_segments(config.final_num_segments.unwrap_or(20))?;
        }

        Ok(())
    }
}
