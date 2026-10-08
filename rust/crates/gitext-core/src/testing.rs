//! Test helpers shared by the crates of the workspace (snapshot verification
//! compatible with the `*.verified.txt` files of the original C# test-suite).

use std::path::PathBuf;

/// Directory `rust/testdata`.
pub fn test_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

/// Reads a test data file, stripping a UTF-8 BOM and normalising line endings.
pub fn read_test_data(relative: &str) -> String {
    let path = test_data_dir().join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    normalize(&text)
}

fn normalize(text: &str) -> String {
    text.trim_start_matches('\u{feff}').replace("\r\n", "\n")
}

/// Compares `actual` with a verified snapshot file. Set `UPDATE_SNAPSHOTS=1` to (re)write it.
pub fn verify(relative: &str, actual: &str) {
    let path = test_data_dir().join(relative);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = read_test_data(relative);
    let actual = normalize(actual);
    assert!(
        expected.trim_end() == actual.trim_end(),
        "snapshot mismatch for {relative}\n--- expected ---\n{expected}\n--- actual ---\n{actual}\n"
    );
}
