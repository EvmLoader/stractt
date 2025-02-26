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

use std::net::SocketAddr;

use crate::sonic;

use super::{Map, Result, Task};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use tracing::{debug, info};

#[derive(Default)]
pub struct StatelessWorker {}

#[async_trait]
pub trait Worker {
    async fn run<I, O>(&self, addr: SocketAddr) -> Result<()>
    where
        Self: Sized,
        I: Map<Self, O> + Send + Sync,
        O: Serialize + DeserializeOwned + Send,
    {
        let server = sonic::Server::bind(addr).await?;
        info!("worker listening on: {:}", addr);

        loop {
            let req: sonic::Request<Task<I>> = server.accept().await?;
            debug!("received request");
            match &req.body {
                Task::Job(job) => {
                    debug!("request is a job");
                    let res = job.map(self);
                    req.respond(sonic::Response::Content(res)).await?;
                }
                Task::AllFinished => {
                    req.respond::<Task<I>>(sonic::Response::Empty).await?;
                    break;
                }
            }
        }

        Ok(())
    }
}

impl Worker for StatelessWorker {}
