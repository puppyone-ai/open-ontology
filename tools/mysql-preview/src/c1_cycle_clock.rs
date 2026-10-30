//! Worker budgets belong to immutable occurrences, not to installation lifetime.
use super::*;

pub(crate) fn deadline(settings:&Value,context:&Value,saved:Option<u64>,now:u64)->Result<u64,PreviewError> {
    let fail=||PreviewError::new("c1","cycle_clock");
    let result=if let Some(clock)=context.get("cycle_clock").filter(|v|!v.is_null()) {
        let started=clock["started_at_ms"].as_u64().ok_or_else(fail)?;
        let timeout=clock["timeout_ms"].as_u64().filter(|n|*n>0).ok_or_else(fail)?;
        let deadline=clock["deadline_at_ms"].as_u64().ok_or_else(fail)?;
        if clock.as_object().is_none_or(|o|o.len()!=9)
            || clock["protocol"]!="oo.jobs.native-cycle-clock/v1"
            || clock["task_id"]!=context["lease_id"] || clock["source_id"]!=context["source_id"]
            || clock["binding"]!=context["binding"] || clock["pin"]!=context["pin"]
            || clock["manifest_sha256"].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit()))
            || started.checked_add(timeout)!=Some(deadline) || now<started {
            return Err(fail());
        }
        deadline
    } else if let Some(saved)=saved {
        if settings.get("cycle_deadline_unix_ms").is_some_and(|v|v.as_u64()!=Some(saved)) {
            return Err(PreviewError::new("c1","typed_resume_deadline_changed"));
        }
        saved
    } else if let Some(absolute)=settings.get("cycle_deadline_unix_ms") {
        absolute.as_u64().ok_or_else(fail)?
    } else {
        now.checked_add(300_000).ok_or_else(fail)?
    };
    if saved.is_some_and(|old|old!=result) {
        return Err(PreviewError::new("c1","typed_resume_deadline_changed"));
    }
    if now>=result {return Err(PreviewError::new("budget","cycle_deadline"));}
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_new_occurrence_budget_does_not_renew_old_saved_deadline() {
        let settings=json!({"cycle_deadline_unix_ms":1100});
        let mut context=json!({"lease_id":"new-task","source_id":"s","binding":"b","pin":2});
        assert!(deadline(&settings,&context,None,2000).is_err());
        context["cycle_clock"]=json!({"protocol":"oo.jobs.native-cycle-clock/v1",
            "task_id":"new-task","source_id":"s","binding":"b","pin":2,
            "manifest_sha256":"a".repeat(64),"timeout_ms":100,"started_at_ms":2000,"deadline_at_ms":2100});
        assert_eq!(deadline(&settings,&context,None,2001).unwrap(),2100);
        assert_eq!(deadline(&settings,&context,Some(2100),2050).unwrap(),2100);
        assert!(deadline(&settings,&context,Some(1100),2050).is_err());
        assert!(deadline(&settings,&context,Some(2100),2100).is_err());
        context["cycle_clock"]["task_id"]=json!("old-task");
        assert!(deadline(&settings,&context,None,2001).is_err());
        assert_eq!(deadline(&json!({}),&json!({}),Some(10000),5000).unwrap(),10000);
        assert!(deadline(&settings,&json!({}),Some(1100),2000).is_err());
    }
}
