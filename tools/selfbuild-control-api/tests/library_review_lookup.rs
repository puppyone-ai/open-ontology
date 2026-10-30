use selfbuild_control_api::{config,store};

#[tokio::test]
async fn review_member_lookup_is_bound_to_both_build_and_exact_object() {
    let path=std::env::var("OO_LIBRARY_REVIEW_CONFIG").expect("owned review fixture config");
    let config=config::load_config_file(&path).unwrap();
    let db=store::connect(&config.postgres).await.unwrap();
    let row=db.query_one("SELECT build_id,object_id FROM oo_selfbuild_api.member ORDER BY build_id,object_id LIMIT 1",&[]).await.unwrap();
    let build:String=row.get(0);let object:String=row.get(1);
    let member=store::find_member(&config.postgres,&build,&object).await.unwrap().unwrap();
    assert_eq!(member.object_id,object);
    assert!(store::find_member(&config.postgres,&build,"app.not_in_scope").await.unwrap().is_none());
    assert!(store::find_member(&config.postgres,"b_unknown_build",&object).await.unwrap().is_none());
}
