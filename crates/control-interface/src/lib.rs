pub mod broker;
mod generated;
mod inv;

pub use crate::generated::ctliface::*;
use actix_rt::time::delay_for;
use futures::stream::StreamExt;
use futures::TryStreamExt;
use inv::{Invocation, InvocationResponse, WasccEntity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use wascap::prelude::KeyPair;

type Result<T> = ::std::result::Result<T, Box<dyn ::std::error::Error + Send + Sync>>;

pub struct Client {
    nc: nats::asynk::Connection,
    nsprefix: Option<String>,
    timeout: Duration,
    key: KeyPair,
}

impl Client {
    pub fn new(nc: nats::asynk::Connection, nsprefix: Option<String>, timeout: Duration) -> Self {
        Client {
            nc,
            nsprefix,
            timeout,
            key: KeyPair::new_server(),
        }
    }

    pub async fn get_hosts(&self, timeout: Duration) -> Result<Vec<Host>> {
        let subject = broker::queries::hosts(&self.nsprefix);

        self.nc
            .request_multi(&subject, vec![])
            .await?
            .map(|m| deserialize::<Host>(&m.data))
            .take_until(delay_for(timeout))
            .try_collect()
            .await
    }

    pub async fn perform_actor_auction(
        &self,
        actor_ref: &str,
        constraints: HashMap<String, String>,
        timeout: Duration,
    ) -> Result<Vec<ActorAuctionAck>> {
        let subject = broker::actor_auction_subject(&self.nsprefix);
        let bytes = serialize(ActorAuctionRequest {
            actor_ref: actor_ref.to_string(),
            constraints,
        })?;
        self.nc
            .request_multi(&subject, bytes)
            .await?
            .map(|m| deserialize::<ActorAuctionAck>(&m.data))
            .take_until(delay_for(timeout))
            .try_collect()
            .await
    }

    pub async fn perform_provider_auction(
        &self,
        provider_ref: &str,
        link_name: &str,
        constraints: HashMap<String, String>,
        timeout: Duration,
    ) -> Result<Vec<ProviderAuctionAck>> {
        let subject = broker::provider_auction_subject(&self.nsprefix);
        let bytes = serialize(ProviderAuctionRequest {
            provider_ref: provider_ref.to_string(),
            link_name: link_name.to_string(),
            constraints,
        })?;
        self.nc
            .request_multi(&subject, bytes)
            .await?
            .map(|m| deserialize::<ProviderAuctionAck>(&m.data))
            .take_until(delay_for(timeout))
            .try_collect()
            .await
    }

    pub async fn get_host_inventory(&self, host_id: &str) -> Result<HostInventory> {
        let subject = broker::queries::host_inventory(&self.nsprefix, host_id);
        match actix_rt::time::timeout(self.timeout, self.nc.request(&subject, vec![])).await? {
            Ok(msg) => {
                let hi: HostInventory = deserialize(&msg.data)?;
                Ok(hi)
            }
            Err(e) => Err(format!("Did not receive host inventory from target host: {}", e).into()),
        }
    }

    pub async fn start_actor(&self, host_id: &str, actor_ref: &str) -> Result<StartActorAck> {
        let subject = broker::commands::start_actor(&self.nsprefix, host_id);
        let bytes = serialize(StartActorCommand {
            actor_ref: actor_ref.to_string(),
            host_id: host_id.to_string(),
        })?;
        match actix_rt::time::timeout(self.timeout, self.nc.request(&subject, &bytes)).await? {
            Ok(msg) => {
                let ack: StartActorAck = deserialize(&msg.data)?;
                Ok(ack)
            }
            Err(e) => Err(format!("Did not receive start actor acknowledgement: {}", e).into()),
        }
    }

    /// Performs a remote procedure call over the lattice, targeting the given actor. This call will appear
    /// to originate from the "system" actor and from a unique host ID that was generated by the control
    /// interface client when it was instantiated. If there are multiple actors with the same public key
    /// actively running in the lattice, then the message broker is responsible for choosing the appropriate
    /// target. Under current NATS implementations, that means an actor is chosen psuedo-randomly among the
    /// known queue subscribers, and will **not** be invoked in round-robin fashion
    pub async fn call_actor(
        &self,
        target_id: &str,
        operation: &str,
        data: &[u8],
    ) -> Result<InvocationResponse> {
        let subject = broker::rpc::call_actor(&self.nsprefix, target_id);
        let bytes = crate::generated::ctliface::serialize(Invocation::new(
            &self.key,
            WasccEntity::Actor("system".to_string()),
            WasccEntity::Actor(target_id.to_string()),
            operation,
            data.to_vec(),
        ))?;
        match actix_rt::time::timeout(self.timeout, self.nc.request(&subject, &bytes)).await? {
            Ok(msg) => {
                let resp: InvocationResponse = crate::generated::ctliface::deserialize(&msg.data)?;
                Ok(resp)
            }
            Err(e) => Err(format!("Actor RPC call did not succeed: {}", e).into()),
        }
    }

    /// Publishes the link advertisement message to the lattice that is published when code invokes the `set_link`
    /// function on a `Host` struct instance. No confirmation or acknowledgement is available for this operation
    /// because it is publish-only.
    pub async fn advertise_link(
        &self,
        actor_id: &str,
        provider_id: &str,
        contract_id: &str,
        link_name: &str,
        values: HashMap<String, String>,
    ) -> Result<()> {
        let subject = broker::rpc::advertise_links(&self.nsprefix);
        let ld = LinkDefinition {
            actor_id: actor_id.to_string(),
            provider_id: provider_id.to_string(),
            contract_id: contract_id.to_string(),
            link_name: link_name.to_string(),
            values,
        };
        let bytes = crate::generated::ctliface::serialize(&ld)?;
        self.nc.publish(&subject, &bytes).await?;

        Ok(())
    }

    /// Issue a command to a host instructing that it replace an existing actor (indicated by its
    /// public key) with a new actor indicated by an OCI image reference. The host will acknowledge
    /// this request as soon as it verifies that the target actor is running. This acknowledgement
    /// occurs **before** the new bytes are downloaded. Live-updating an actor can take a long
    /// time and control clients cannot block waiting for a reply that could come several seconds
    /// later. If you need to verify that the actor has been updated, you will want to set up a
    /// listener for the appropriate **ControlEvent** which will be published on the control events
    /// channel in JSON
    pub async fn update_actor(
        &self,
        host_id: &str,
        existing_actor_id: &str,
        new_actor_ref: &str,
    ) -> Result<UpdateActorAck> {
        let subject = broker::commands::update_actor(&self.nsprefix, host_id);
        let bytes = serialize(UpdateActorCommand {
            host_id: host_id.to_string(),
            actor_id: existing_actor_id.to_string(),
            new_actor_ref: new_actor_ref.to_string(),
        })?;
        match actix_rt::time::timeout(self.timeout, self.nc.request(&subject, &bytes)).await? {
            Ok(msg) => {
                let ack: UpdateActorAck = deserialize(&msg.data)?;
                Ok(ack)
            }
            Err(e) => Err(format!("Did not receive update actor acknowledgement: {}", e).into()),
        }
    }

    pub async fn start_provider(
        &self,
        host_id: &str,
        provider_ref: &str,
        link_name: Option<String>,
    ) -> Result<StartProviderAck> {
        let subject = broker::commands::start_provider(&self.nsprefix, host_id);
        let bytes = serialize(StartProviderCommand {
            host_id: host_id.to_string(),
            provider_ref: provider_ref.to_string(),
            link_name: link_name.unwrap_or("default".to_string()),
        })?;
        match actix_rt::time::timeout(self.timeout, self.nc.request(&subject, &bytes)).await? {
            Ok(msg) => {
                let ack: StartProviderAck = deserialize(&msg.data)?;
                Ok(ack)
            }
            Err(e) => Err(format!("Did not receive start provider acknowledgement: {}", e).into()),
        }
    }

    pub async fn stop_provider(
        &self,
        host_id: &str,
        provider_ref: &str,
        link_name: &str,
        contract_id: &str,
    ) -> Result<StopProviderAck> {
        let subject = broker::commands::stop_provider(&self.nsprefix, host_id);
        let bytes = serialize(StopProviderCommand {
            host_id: host_id.to_string(),
            provider_ref: provider_ref.to_string(),
            link_name: link_name.to_string(),
            contract_id: contract_id.to_string(),
        })?;
        match actix_rt::time::timeout(self.timeout, self.nc.request(&subject, &bytes)).await? {
            Ok(msg) => {
                let ack: StopProviderAck = deserialize(&msg.data)?;
                Ok(ack)
            }
            Err(e) => Err(format!("Did not receive stop provider acknowledgement: {}", e).into()),
        }
    }

    pub async fn stop_actor(&self, host_id: &str, actor_ref: &str) -> Result<StopActorAck> {
        let subject = broker::commands::stop_actor(&self.nsprefix, host_id);
        let bytes = serialize(StopActorCommand {
            host_id: host_id.to_string(),
            actor_ref: actor_ref.to_string(),
        })?;
        match actix_rt::time::timeout(self.timeout, self.nc.request(&subject, &bytes)).await? {
            Ok(msg) => {
                let ack: StopActorAck = deserialize(&msg.data)?;
                Ok(ack)
            }
            Err(e) => Err(format!("Did not receive stop actor acknowledgement: {}", e).into()),
        }
    }

    pub async fn get_claims(&self) -> Result<ClaimsList> {
        let subject = broker::queries::claims(&self.nsprefix);
        match actix_rt::time::timeout(self.timeout, self.nc.request(&subject, vec![])).await? {
            Ok(msg) => {
                let list: ClaimsList = deserialize(&msg.data)?;
                Ok(list)
            }
            Err(e) => Err(format!("Did not receive claims from lattice: {}", e).into()),
        }
    }
}

/// The standard function for serializing codec structs into a format that can be
/// used for message exchange between actor and host. Use of any other function to
/// serialize could result in breaking incompatibilities.
pub fn serialize<T>(
    item: T,
) -> ::std::result::Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>
where
    T: Serialize,
{
    serde_json::to_vec(&item).map_err(|_e| "JSON serialization failure".into())
}

/// The standard function for de-serializing codec structs from a format suitable
/// for message exchange between actor and host. Use of any other function to
/// deserialize could result in breaking incompatibilities.
pub fn deserialize<'de, T: Deserialize<'de>>(
    buf: &'de [u8],
) -> ::std::result::Result<T, Box<dyn std::error::Error + Send + Sync>> {
    serde_json::from_slice(buf).map_err(|_e| "JSON deserialization failure".into())
}
