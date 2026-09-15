//! Opt-in Linux registration. A session owns its bindings until reconfiguration.
use super::*;
use kw_platform::portal_shortcuts::{linux_impl, trigger_for};
use std::sync::{Arc, Mutex};

type Reports = Arc<Mutex<Vec<RegistrationReport>>>;

struct Generation {
    counter: Arc<std::sync::atomic::AtomicU64>,
    mine: u64,
}
impl Generation {
    fn current(&self) -> bool {
        self.counter.load(std::sync::atomic::Ordering::SeqCst) == self.mine
    }
}

fn publish(target: &Reports, reports: Vec<RegistrationReport>, generation: &Generation) {
    let mut target = target
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if generation.current() {
        *target = reports;
    }
}

pub(super) fn register(
    app: &AppHandle,
    bindings: &[Binding],
    hooks: &Hooks,
) -> Vec<RegistrationReport> {
    let gate = Arc::clone(&hooks.portal_gate);
    let registration_guard = gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let generation = Generation {
        counter: Arc::clone(&hooks.portal_generation),
        mine: hooks
            .portal_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1,
    };
    let mut task = hooks
        .portal_task
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(previous) = task.take() {
        previous.abort();
    }
    crate::dictation::release_the_hold();
    let cleanup_error = app.global_shortcut().unregister_all().err().map(|error| {
        format!("Could not release the previous shortcuts; restart the application: {error}")
    });
    let mut routes = Vec::new();
    let mut reports = Vec::new();
    for binding in bindings {
        let mut results = Vec::new();
        for accelerator in &binding.accelerators {
            let text = accelerator.text().to_owned();
            let result = if let Some(reason) = unhandled_reason(binding.action) {
                Registration::Unhandled {
                    accelerator: text,
                    reason: reason.to_owned(),
                }
            } else if let Some(reason) = &cleanup_error {
                Registration::Rejected {
                    accelerator: text,
                    reason: reason.clone(),
                }
            } else if let Err(reason) = trigger_for(&text) {
                Registration::NotRegistrable {
                    accelerator: text,
                    reason: reason.to_string(),
                }
            } else if routes.iter().any(|(_, existing, _)| existing == &text) {
                Registration::Rejected {
                    accelerator: text,
                    reason: "Duplicate shortcut; choose a distinct accelerator.".into(),
                }
            } else {
                routes.push((binding.action, text.clone(), binding.mode));
                Registration::Rejected {
                    accelerator: text,
                    reason: "Waiting for the desktop shortcut portal.".into(),
                }
            };
            results.push(result);
        }
        reports.push(RegistrationReport {
            action: binding.action,
            results,
        });
    }
    hooks.set_reports(reports.clone());
    if routes.is_empty() {
        return reports;
    }
    let initial = reports.clone();
    let target = Arc::clone(&hooks.reports);
    let app = app.clone();
    drop(registration_guard);
    *task = Some(tauri::async_runtime::spawn(async move {
        let accelerators: Vec<_> = routes.iter().map(|(_, text, _)| text.clone()).collect();
        let descriptions: Vec<_> = routes
            .iter()
            .map(|(action, _, _)| format!("KnegisWhisper {action}"))
            .collect();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            linux_impl::bind(&accelerators, &descriptions),
        )
        .await;
        let mut current = initial;
        match result {
            Ok(Ok((session, _))) => {
                if !generation.current() {
                    return;
                }
                for (_, accelerator, _) in &routes {
                    update(
                        &mut current,
                        accelerator,
                        Registration::Rejected {
                            accelerator: accelerator.clone(),
                            reason: "The desktop did not grant this shortcut.".into(),
                        },
                    );
                }
                for bound in session.bound() {
                    update(
                        &mut current,
                        &bound.accelerator,
                        Registration::Portal {
                            accelerator: bound.accelerator.clone(),
                            granted: bound.granted.clone(),
                        },
                    );
                }
                publish(&target, current.clone(), &generation);
                let changes = Arc::clone(&target);
                let delivery = session
                    .deliver_with_changes(
                        |index, edge| {
                            let _guard = gate
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if !generation.current() {
                                return;
                            }
                            if let Some((action, accelerator, mode)) = routes.get(index) {
                                let what = act(edge, *mode);
                                if does_something(what) {
                                    let _ = app.emit(
                                        HOTKEY_EVENT,
                                        HotkeyFired {
                                            action: *action,
                                            accelerator: accelerator.clone(),
                                        },
                                    );
                                    if matches!(dispatch(*action), Dispatch::Dictation) {
                                        crate::dictation::on_hotkey(&app, what);
                                    }
                                }
                            }
                        },
                        |bound| {
                            if !generation.current() {
                                return;
                            }
                            let mut reports = changes
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if !generation.current() {
                                return;
                            }
                            for item in bound {
                                update(
                                    &mut reports,
                                    &item.accelerator,
                                    Registration::Portal {
                                        accelerator: item.accelerator.clone(),
                                        granted: item.granted.clone(),
                                    },
                                );
                            }
                        },
                    )
                    .await;
                if !generation.current() {
                    return;
                }
                crate::dictation::release_the_hold();
                let reason = format!(
                    "Desktop shortcut portal disconnected; restart the application. {}",
                    delivery.err().map(|e| e.to_string()).unwrap_or_default()
                );
                for (_, accelerator, _) in &routes {
                    update(
                        &mut current,
                        accelerator,
                        Registration::Rejected {
                            accelerator: accelerator.clone(),
                            reason: reason.clone(),
                        },
                    );
                }
                publish(&target, current, &generation);
            }
            Ok(Err(error)) if error.can_fallback() => {
                let _guard = gate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !generation.current() {
                    return;
                }
                for (action, accelerator, mode) in &routes {
                    update(
                        &mut current,
                        accelerator,
                        register_one(&app, *action, accelerator, *mode),
                    );
                }
                publish(&target, current, &generation);
            }
            other => {
                let reason = match other {
                    Ok(Err(e)) => e.to_string(),
                    _ => "Desktop shortcut request timed out; restart to try again.".into(),
                };
                for (_, accelerator, _) in &routes {
                    update(
                        &mut current,
                        accelerator,
                        Registration::Rejected {
                            accelerator: accelerator.clone(),
                            reason: reason.clone(),
                        },
                    );
                }
                publish(&target, current, &generation);
            }
        }
    }));
    reports
}

fn update(reports: &mut [RegistrationReport], accelerator: &str, result: Registration) {
    for report in reports {
        for registration in &mut report.results {
            if matches!(registration, Registration::Unhandled { .. }) {
                continue;
            }
            let current = match registration {
                Registration::Portal { accelerator, .. }
                | Registration::Registered { accelerator }
                | Registration::Hooked { accelerator }
                | Registration::Rejected { accelerator, .. }
                | Registration::NotRegistrable { accelerator, .. }
                | Registration::Unhandled { accelerator, .. } => accelerator,
            };
            if current == accelerator {
                *registration = result.clone();
            }
        }
    }
}
