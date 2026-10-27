use super::*;
use crate::protocol::wire::serialization::js_truthy;
use rand::seq::SliceRandom;

pub(super) struct Common {
    build: SdkBuild,
    revision: u64,
    bytes: Vec<u8>,
}
pub(super) struct PreparedReport {
    revision: u64,
    packet: String,
}
impl WatchmanRuntime {
    pub(super) fn capture_common(&self, snapshot: &ConfigSnapshot) -> Result<Common> {
        let state = DeviceStore::new(self.store.clone(), snapshot.config().clone());
        let prior = state.snapshot()?;
        let mut random = [0; 31];
        self.clock.fill(&mut random)?;
        let mut common = {
            let mut p = self.public()?;
            p.sequence = p.sequence.saturating_add(1);
            public_fields_for(
                snapshot.config().build(),
                CommonFields {
                    product: snapshot.config().product(),
                    business: p.business.as_deref(),
                    nonce: p.nonce.as_deref(),
                    tid: prior.tid_native(),
                    did: prior.did_native(),
                    track: p.track.as_deref(),
                    local_ms: self.clock.now_ms()?,
                    server_offset_ms: prior.clock_offset_ms(),
                    sequence: p.sequence,
                },
                random,
            )?
        };
        common.shuffle(&mut rand::rng());
        Ok(Common {
            build: snapshot.config().build(),
            revision: prior.revision(),
            bytes: common.concat(),
        })
    }
    pub(super) fn prepare_report(
        &self,
        common: Common,
        kind: ReportKind,
        observed: &Observations,
    ) -> Result<PreparedReport> {
        let mut payload = common.bytes;
        let groups: Value = serde_json::from_str(include_str!("../collectors.json"))?;
        let table = descriptors()?;
        for (group, values, shuffle) in [
            ("device", &observed.device, true),
            ("environment", &observed.environment, true),
            ("events", &observed.events, false),
        ] {
            let allowed = groups[group]
                .as_array()
                .ok_or(Error::State("invalid Watchman collector table"))?;
            let mut fields = Vec::new();
            let mut seen = std::collections::BTreeSet::new();
            for fact in values {
                if !allowed.iter().any(|k| k.as_str() == Some(&fact.key))
                    || (group != "events" && !seen.insert(&fact.key))
                {
                    return Err(Error::InvalidInput(
                        "Watchman collector supplied an invalid or duplicate field",
                    ));
                }
                let descriptor = table
                    .iter()
                    .find(|d| d.key == fact.key)
                    .ok_or(Error::InvalidInput("unknown Watchman observation field"))?;
                fields.push(encode_field(descriptor, &fact.value, fact.failed)?);
            }
            if shuffle && !self.collection.preserves_field_order() {
                fields.shuffle(&mut rand::rng());
            }
            payload.extend(fields.concat());
        }
        if kind == ReportKind::Device && !observed.events.is_empty() {
            return Err(Error::InvalidInput(
                "device report must not drain behavior events",
            ));
        }
        let mut seed = [0; 4];
        self.clock.fill(&mut seed)?;
        let started = std::time::Instant::now();
        let packet = encode_packet_for(common.build, &payload, seed)?;
        self.collection.record_timing(
            TimingSlot::Packet,
            started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        )?;
        Ok(PreparedReport {
            revision: common.revision,
            packet,
        })
    }
    pub(super) async fn report(
        &self,
        snapshot: &ConfigSnapshot,
        kind: ReportKind,
        prepared: &PreparedReport,
    ) -> Result<Option<i64>> {
        let mut response = None;
        for &host in snapshot.hosts() {
            let mut random = [0; 31];
            self.clock.fill(&mut random)?;
            let request = Request {
                host,
                callback: format!("__wmjsonp_{}", &sdk_uuid(random)[2..9]),
                kind: RequestKind::Report {
                    build: snapshot.config().build(),
                    kind,
                    packet: prepared.packet.clone(),
                },
            };
            if let Ok(value) = self.transport.send(request).await {
                response = Some(value);
                break;
            }
        }
        let Some(response) = response else {
            return Ok(None);
        };
        let code = response
            .as_array()
            .and_then(|v| v.first())
            .and_then(Value::as_i64);
        if matches!(code, Some(200 | 420)) {
            let correction = if code == Some(420) && js_truthy(&response[1]) {
                Some(parse_server_time(&response[1])?)
            } else {
                None
            };
            let _open = self.open()?;
            DeviceStore::new(self.store.clone(), snapshot.config().clone()).apply_response(
                prepared.revision,
                kind,
                &response,
                self.clock.now_ms()?,
                correction,
            )?;
        }
        Ok(code)
    }
}
