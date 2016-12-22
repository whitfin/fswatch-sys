use std;
use std::sync::{Arc, Mutex, Condvar};
use {ffi, FswSession, FswStatus, FswError, FswMonitorType, FswMonitorFilter, FswFilterType,
     FswEventFlag, FswEvent};

fn get_default_session() -> FswSession {
  FswSession::default().unwrap()
}

fn create_sample_filter() -> FswMonitorFilter {
  FswMonitorFilter::new("\\w+\\.txt$", FswFilterType::Include, false, false)
}

#[test]
fn create_and_destroy_session() {
  let handle = {
    let session = get_default_session();
    session.handle
  };
  // Check that the handle was created successfully.
  assert!(handle != ffi::FSW_INVALID_HANDLE);
  // Check that trying to destroy the handle after the session wrapper goes out of scope fails.
  // This should fail because the wrapper going out of scope should automatically destroy the
  // session.
  assert!(unsafe { ffi::fsw_destroy_session(handle) } != ffi::FSW_OK);
}

#[test]
fn create_session_from_builder() {
  FswSession::builder()
    .add_path("./")
    .property("test_name", "test_value")
    .overflow(Some(true))
    .monitor(FswMonitorType::SystemDefault)
    .latency(Some(1.0))
    .recursive(Some(true))
    .directory_only(Some(false))
    .follow_symlinks(Some(true))
    .add_event_filter(FswEventFlag::Created)
    .add_filter(create_sample_filter())
    .build_callback(|events| println!("{:#?}", events))
    .unwrap();
}

#[test]
fn start_empty() {
  assert_eq!(Err(FswError::MissingRequiredParameters), get_default_session().start_monitor());
}

#[test]
fn start_without_callback() {
  let session = get_default_session();
  session.add_path("./").unwrap();
  assert_eq!(Err(FswError::MissingRequiredParameters), session.start_monitor());
}

#[test]
fn start_without_path() {
  let session = get_default_session();
  session.set_callback(|_| println!("Hello")).unwrap();
  assert_eq!(Err(FswError::MissingRequiredParameters), session.start_monitor());
}

#[test]
fn invalid_path() {
  let session = get_default_session();
  unsafe {
    assert!(ffi::fsw_add_path(session.handle, ::std::ptr::null()) == ffi::FSW_ERR_INVALID_PATH);
  }
}

#[test]
fn invalid_handle_add_path() {
  let mut session = get_default_session();
  // Set handle to the invalid handle before trying to call methods.
  session.handle = ffi::FSW_INVALID_HANDLE;
  let res = session.add_path("./");
  let expected_error = Err(FswError::FromFsw(FswStatus::SessionUnknown));
  assert_eq!(expected_error, res);
  assert!(!session.path_added.get());
}

#[test]
fn invalid_handle_set_callback() {
  let mut session = get_default_session();
  // Set handle to the invalid handle before trying to call methods.
  session.handle = ffi::FSW_INVALID_HANDLE;
  let res = session.set_callback(|_| {});
  let expected_error = Err(FswError::FromFsw(FswStatus::SessionUnknown));
  assert_eq!(expected_error, res);
  assert!(!session.callback_set.get());
}

#[test]
fn run_callback() {
  // Define the file name for this test.
  let file_name = "fsw_test_file";

  // Create new condvar for waiting.
  let pair = Arc::new((Mutex::new(false), Condvar::new()));
  // Create clone for thread.
  let pair2 = pair.clone();

  // Get a handle to this thread.
  let handle = std::thread::spawn(move || {
    // Extract our pair.
    let &(ref lock, ref cvar) = &*pair2;
    // Create a session.
    let session = FswSession::builder_paths(vec!["./"])
      // Filter for only our file name.
      .add_filter(FswMonitorFilter::new(file_name, FswFilterType::Include, true, false))
      // Reject all other files.
      .add_filter(FswMonitorFilter::new(".*", FswFilterType::Exclude, false, false))
      .build().unwrap();
    // Use the iterator pattern but immediately break out of it. This will leave the monitor running
    // and accumulating events (as the C API provides no way to stop a monitor).
    for event in session {
      // Once we get an event, notify our waiting condvar and return the event.
      let mut started = lock.lock().unwrap();
      *started = true;
      cvar.notify_one();
      return event;
    }
    // This should be unreachable code, so panic if we get here.
    unreachable!();
  });

  // Create the file for the thread to find the event for.
  let file_path = format!("./{}", file_name);
  std::fs::File::create(&file_path).unwrap();

  // Wait for the thread for up to five seconds.
  let &(ref lock, ref cvar) = &*pair;
  let mut started = lock.lock().unwrap();
  while !*started {
    let (s, timeout) = cvar.wait_timeout(started, std::time::Duration::from_secs(5)).unwrap();
    started = s;
    // Assert that we didn't time out waiting. This prevents the test from infinitely blocking.
    assert!(!timeout.timed_out());
  }

  // Get the event from the thread.
  let event: FswEvent = handle.join().unwrap();

  // Remove the file.
  std::fs::remove_file(&file_path).unwrap();

  let path = std::path::PathBuf::from(event.path);
  let event_file_name = path.file_name().unwrap().to_string_lossy();
  // Assert that the created file name matches the event's file name.
  assert_eq!(file_name, event_file_name);
  // Assert that the IsFile flag is present in the event flags. Note that we cannot assert that the
  // Created flag is present due to how fswatch works.
  assert!(event.flags.contains(&FswEventFlag::IsFile));
}
