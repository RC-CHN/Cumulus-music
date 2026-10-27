use super::*;

/// Read the original plan/journal, never create again, and delete only its
/// independently confirmed empty private fixture. Unknown cleanup is read only.
pub async fn recover(
    original: &Path,
    credentials: Arc<Credentials>,
    profile: ClientProfile,
    context: Arc<RequestContext>,
) -> Result<Value> {
    let plan: Plan = serde_json::from_slice(&fs::read(original.join("write-plan.json"))?)?;
    if !original.join("write-state/core.sqlite3").is_file() {
        return Err(Error::State("original write journal is missing"));
    }
    let root = recovery_directory(original)?;
    let run = Run::open(original, &plan.name, true, credentials, profile, context).await?;
    if run.token.uid.as_deref() != Some(plan.owner.as_str()) {
        return Err(Error::State("recovery account differs from original plan"));
    }
    let mut records = run.service.operations(&run.token)?;
    if records.len() > 1
        || records.iter().any(|r| {
            !matches!(
                &r.command, PlaylistCommand::Create { name, private:true } if name == &plan.name
            )
        })
    {
        return Err(Error::State(
            "write proof journal differs from original plan",
        ));
    }
    let result = finish(&root, &plan, &run, records.pop()).await;
    if let Err(e) = &result {
        save_new(
            &root,
            "recovery-error.json",
            &json!({"error":e.to_string()}),
        )?;
    }
    result
}

fn recovery_directory(original: &Path) -> Result<PathBuf> {
    for i in 0..100 {
        let path = original.join(format!("write-recovery-{i}"));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(Error::State("write recovery history limit reached"))
}
