//! The watchdog judges only workers that are expected to run, and never
//! mistakes a frozen process for a hung worker.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_engine::media::{Heartbeat, WorkerFailure, WorkerId, Watched, watchdog};

type Reports = Arc<Mutex<Vec<WorkerFailure>>>;

fn start(heartbeat: &Arc<Heartbeat>) -> (Reports, Arc<AtomicBool>, tokio::task::JoinHandle<()>) {
	let reports: Reports = Arc::default();
	let stop = Arc::new(AtomicBool::new(false));
	let task = tokio::spawn(watchdog(
		vec![Watched {
			id: WorkerId::Render,
			heartbeat: Arc::clone(heartbeat),
			failed: Arc::new(AtomicBool::new(false)),
		}],
		Arc::clone(&stop),
		{
			let reports = Arc::clone(&reports);
			Arc::new(move |f| reports.lock().unwrap().push(f))
		},
		Duration::from_millis(10),
		Duration::from_millis(100),
	));
	(reports, stop, task)
}

#[tokio::test]
async fn an_idle_silent_worker_is_never_reported() {
	let heartbeat = Arc::new(Heartbeat::default());
	heartbeat.set_active(false);
	let (reports, stop, task) = start(&heartbeat);
	tokio::time::sleep(Duration::from_millis(500)).await;
	assert!(reports.lock().unwrap().is_empty());
	stop.store(true, Ordering::Release);
	task.await.unwrap();
}

#[tokio::test]
async fn an_active_worker_that_stops_beating_is_reported() {
	let heartbeat = Arc::new(Heartbeat::default());
	heartbeat.set_active(true);
	let (reports, stop, task) = start(&heartbeat);
	tokio::time::sleep(Duration::from_millis(400)).await;
	assert_eq!(reports.lock().unwrap().len(), 1);
	stop.store(true, Ordering::Release);
	task.await.unwrap();
}

#[tokio::test]
async fn a_worker_that_goes_idle_before_the_deadline_is_not_reported() {
	let heartbeat = Arc::new(Heartbeat::default());
	heartbeat.set_active(true);
	let (reports, stop, task) = start(&heartbeat);
	tokio::time::sleep(Duration::from_millis(50)).await;
	heartbeat.set_active(false);
	tokio::time::sleep(Duration::from_millis(400)).await;
	assert!(reports.lock().unwrap().is_empty());
	stop.store(true, Ordering::Release);
	task.await.unwrap();
}

#[test]
fn a_frozen_process_is_not_a_hung_worker() {
	// Block the only runtime thread so the watchdog itself wakes late, as it
	// does when the system freezes the app; the worker could not beat either.
	let rt = tokio::runtime::Builder::new_current_thread()
		.enable_time()
		.build()
		.unwrap();
	rt.block_on(async {
		let heartbeat = Arc::new(Heartbeat::default());
		heartbeat.set_active(true);
		let (reports, stop, task) = start(&heartbeat);
		tokio::time::sleep(Duration::from_millis(30)).await;
		std::thread::sleep(Duration::from_millis(300));
		tokio::time::sleep(Duration::from_millis(30)).await;
		assert!(
			reports.lock().unwrap().is_empty(),
			"a late watchdog must give the worker a fresh deadline"
		);
		stop.store(true, Ordering::Release);
		task.await.unwrap();
	});
}
