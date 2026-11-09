//! Same-machine delivery prompt; it deliberately grants no new access.
pub fn render(profile: &str) -> Result<String, &'static str> {
    if !crate::profile::valid_name(profile) {
        return Err("PROFILE_NAME");
    }
    Ok(format!(
        "Use the local Open Ontology profile {profile}.\nRun oo use {profile}, then oo check --json and oo agent-skill.\nInspect oo tables and oo describe before writing SQL. Query only through the Gateway.\nCite query_id, publication and freshness. Report NULL, truncation and failures honestly; never substitute zero.\nThis profile is local to this machine; this prompt contains no credentials and does not grant access on another machine.\n"
    ))
}
