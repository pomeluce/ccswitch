use tempfile::tempdir;
use ccswitch::db::Db;

#[test]
fn test_db_open_and_migrate() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Db::open(&db_path).unwrap();
    // Should not panic
    db.set_setting("test_key", "test_value").unwrap();
    assert_eq!(db.get_setting("test_key"), Some("test_value".to_string()));
}