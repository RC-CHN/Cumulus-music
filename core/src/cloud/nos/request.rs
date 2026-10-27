use super::*;
use crate::cloud::CloudApi;
use serde_json::Value;
impl NosUploads {
    pub(super) async fn renew(&self, job: &mut Job) -> Result<()> {
        self.guard()?;
        let attempts = job
            .renewal
            .as_ref()
            .map_or(0, |r| r.attempts)
            .checked_add(1)
            .ok_or(Error::State("NOS renewal counter exhausted"))?;
        job.renewal = Some(Renewal {
            status: NosRenewal::Pending,
            attempts,
        });
        self.save(job)?;
        let bucket = job.allocation.bucket.clone();
        let key = job.allocation.key.clone();
        let result = self
            .session
            .run_for(&self.token, |api, _| async move {
                CloudApi::new(api).refresh_nos_token(&bucket, &key).await
            })
            .await;
        self.guard()?;
        match result {
            Ok(token) => {
                types::valid_token(&token)?;
                job.allocation.token = token;
                job.renewal.as_mut().expect("renewal").status = NosRenewal::Ready;
                self.save(job)
            }
            Err(error) => {
                job.renewal.as_mut().expect("renewal").status = NosRenewal::Failed;
                self.save(job)?;
                Err(error)
            }
        }
    }
    pub(super) async fn send(
        &self,
        job: &mut Job,
        data: Option<Vec<u8>>,
        renewed: &mut bool,
    ) -> Result<Value> {
        for attempt in 0..2 {
            self.guard()?;
            let result = self
                .backend
                .request(NosRequest::new(job, data.clone())?)
                .await;
            self.guard()?;
            if let Err(Error::HttpStatus {
                status: 401 | 403, ..
            }) = &result
            {
                let pending = job.pending.clone();
                if data.is_some() {
                    job.pending = None;
                    self.save(job)?;
                    if self.apply_control(job)? {
                        return Err(Error::Cancelled);
                    }
                }
                let can_renew = job
                    .renewal
                    .as_ref()
                    .is_none_or(|r| r.status == NosRenewal::Confirmed);
                if attempt == 0 && !*renewed && can_renew {
                    *renewed = true;
                    self.renew(job).await?;
                    if data.is_some() {
                        let pending =
                            pending.ok_or(Error::State("missing rejected chunk intent"))?;
                        if !self.prepare_chunk(job, pending)? {
                            return Err(Error::Cancelled);
                        }
                    }
                    continue;
                }
            }
            let result = result?;
            if let Some(renewal) = &mut job.renewal
                && renewal.status == NosRenewal::Ready
            {
                renewal.status = NosRenewal::Confirmed;
                self.save(job)?;
            }
            return Ok(result);
        }
        Err(Error::State("NOS request exhausted renewal budget"))
    }
}
