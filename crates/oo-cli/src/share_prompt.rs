//! Same-machine delivery prompt; it deliberately grants no new access.
pub fn render(profile: &str) -> Result<String, &'static str> {
    render_localized(profile, crate::tui::theme::lang_zh())
}

pub fn render_localized(profile: &str, zh: bool) -> Result<String, &'static str> {
    if !crate::profile::valid_name(profile) {
        return Err("PROFILE_NAME");
    }
    if zh {
        return Ok(format!(
            "使用本机 Open Ontology 配置 {profile}。\n先运行 oo use {profile}，再运行 oo check --json 和 oo agent-skill。\n编写 SQL 前先查看 oo tables 和 oo describe；仅通过 Gateway 查询。\n引用 query_id、发布版本和新鲜度；如实报告 NULL、截断和失败，不得以零替代。\n此配置仅适用于本机；提示词不含凭据，也不授予其他机器访问权限。\n"
        ));
    }
    Ok(format!(
        "Use the local Open Ontology profile {profile}.\nRun oo use {profile}, then oo check --json and oo agent-skill.\nInspect oo tables and oo describe before writing SQL. Query only through the Gateway.\nCite query_id, publication and freshness. Report NULL, truncation and failures honestly; never substitute zero.\nThis profile is local to this machine; this prompt contains no credentials and does not grant access on another machine.\n"
    ))
}
