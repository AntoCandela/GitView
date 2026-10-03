//! Exercises bounded process output.

use super::*;

#[tokio::test]
async fn each_output_stream_accepts_the_limit_and_rejects_the_next_byte() {
    let bytes = vec![b'x'; OUTPUT_LIMIT];
    assert_eq!(read_limited(&mut bytes.as_slice(), OUTPUT_LIMIT).await.unwrap(), bytes);
    let overflow = vec![b'x'; OUTPUT_LIMIT + 1];
    assert!(matches!(
        read_limited(&mut overflow.as_slice(), OUTPUT_LIMIT).await,
        Err(ProcessFailure::OutputLimit)
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn larger_index_enumeration_budget_does_not_raise_other_stream_limits() {
    let directory = tempfile::tempdir().unwrap();
    let executable = crate::test_support::executable(directory.path(), "dd if=/dev/zero bs=1048577 count=1 2>/dev/null");
    let process = GitProcess::with_executable(&executable);
    assert!(matches!(process.run(None, &[], ProbeDeadline::new()).await, Err(ProcessError { failure: ProcessFailure::OutputLimit, .. })));
    let output = process.run_isolated_with_output_limit(None, &[], ProbeDeadline::new(), 2 * OUTPUT_LIMIT).await.unwrap();
    assert_eq!(output.stdout.len(), OUTPUT_LIMIT + 1);
    let executable = crate::test_support::executable(directory.path(), "dd if=/dev/zero bs=1048577 count=1 1>&2 2>/dev/null");
    let process = GitProcess::with_executable(&executable);
    assert!(matches!(process.run_isolated_with_output_limit(None, &[], ProbeDeadline::new(), 2 * OUTPUT_LIMIT).await, Err(ProcessError { failure: ProcessFailure::OutputLimit, .. })));
}
