#[cfg(any(feature = "ssr", test))]
use leptos::prelude::*;
#[cfg(feature = "ssr")]
use leptos::{
    config::LeptosOptions,
    hydration::{AutoReload, HydrationScripts},
};
use symphony_core::{RetrySnapshotRow, RunningSnapshotRow, RuntimeSnapshot};

#[cfg(feature = "hydrate")]
use gloo_net::http::Request;
#[cfg(feature = "hydrate")]
use wasm_bindgen::{JsCast, closure::Closure};
#[cfg(feature = "hydrate")]
use wasm_bindgen_futures::spawn_local;

#[cfg(any(feature = "ssr", test))]
pub(crate) const DASHBOARD_ROOT_ID: &str = "symphony-dashboard-root";
#[cfg(any(feature = "ssr", test))]
const DASHBOARD_STATE_SCRIPT_ID: &str = "symphony-dashboard-snapshot";
#[cfg(any(feature = "ssr", test))]
const DASHBOARD_CONTROLS_ROOT_ID: &str = "symphony-dashboard-controls-root";
const DASHBOARD_GENERATED_AT_ID: &str = "symphony-dashboard-generated-at";
const DASHBOARD_REFRESH_ID: &str = "symphony-dashboard-refresh";
const DASHBOARD_LIVE_STATUS_ID: &str = "symphony-dashboard-live-status";
const DASHBOARD_RUNNING_COUNT_ID: &str = "symphony-dashboard-running-count";
const DASHBOARD_RETRYING_COUNT_ID: &str = "symphony-dashboard-retrying-count";
const DASHBOARD_TOTAL_TOKENS_ID: &str = "symphony-dashboard-total-tokens";
const DASHBOARD_INPUT_TOKENS_ID: &str = "symphony-dashboard-input-tokens";
const DASHBOARD_OUTPUT_TOKENS_ID: &str = "symphony-dashboard-output-tokens";
const DASHBOARD_RUNTIME_SECONDS_ID: &str = "symphony-dashboard-runtime-seconds";
const DASHBOARD_RUNNING_ROWS_ID: &str = "symphony-dashboard-running-rows";
const DASHBOARD_RETRY_ROWS_ID: &str = "symphony-dashboard-retry-rows";
const DASHBOARD_RATE_LIMITS_ID: &str = "symphony-dashboard-rate-limits";

#[cfg(any(feature = "ssr", test))]
const HYDRATION_PENDING_STATUS: &str = "Hydration pending";
#[cfg(feature = "hydrate")]
const LIVE_READY_STATUS: &str = "Live dashboard ready";
#[cfg(feature = "hydrate")]
const LIVE_SYNCING_STATUS: &str = "Synchronizing live dashboard…";
#[cfg(feature = "hydrate")]
const MANUAL_REFRESHING_STATUS: &str = "Refreshing dashboard…";
#[cfg(feature = "hydrate")]
const MANUAL_REFRESHED_STATUS: &str = "Dashboard updated from live state";

#[cfg(any(feature = "ssr", test))]
#[component]
fn DashboardControls() -> impl IntoView {
    view! {
        <div
            id=DASHBOARD_CONTROLS_ROOT_ID
            style="display: flex; flex-direction: column; gap: 0.5rem; align-items: end;"
        >
            <button
                id=DASHBOARD_REFRESH_ID
                type="button"
                data-testid="dashboard-refresh"
                style="padding: 0.7rem 1rem; border-radius: 0.6rem; border: 1px solid #0f172a; background: #0f172a; color: white; font-weight: 600; cursor: pointer;"
            >
                "Refresh dashboard"
            </button>
            <p
                id=DASHBOARD_LIVE_STATUS_ID
                role="status"
                style="margin: 0; color: #475569;"
                data-testid="dashboard-live-status"
            >
                {HYDRATION_PENDING_STATUS}
            </p>
        </div>
    }
}

#[cfg(any(feature = "ssr", test))]
#[component]
fn DashboardApp(snapshot: RuntimeSnapshot) -> impl IntoView {
    let generated_at = render_generated_at(&snapshot);
    let running_count = snapshot.counts.running.to_string();
    let retrying_count = snapshot.counts.retrying.to_string();
    let total_tokens = snapshot.codex_totals.total_tokens.to_string();
    let input_tokens = format!("input: {}", snapshot.codex_totals.input_tokens);
    let output_tokens = format!("output: {}", snapshot.codex_totals.output_tokens);
    let runtime_seconds = format!(
        "runtime seconds: {:.1}",
        snapshot.codex_totals.seconds_running
    );
    let running_rows_html = format_running_rows_html(&snapshot.running);
    let retry_rows_html = format_retry_rows_html(&snapshot.retrying);
    let rate_limits_text = format_rate_limits(snapshot.rate_limits.as_ref());

    view! {
        <main id=DASHBOARD_ROOT_ID style="font-family: ui-sans-serif, system-ui, sans-serif; max-width: 1100px; margin: 2rem auto; padding: 0 1rem;">
            <header style="display: flex; justify-content: space-between; gap: 1rem; align-items: start; flex-wrap: wrap;">
                <div>
                    <h1 style="font-size: 2rem; margin-bottom: 0.5rem;">"Symphony Runtime"</h1>
                    <p
                        id=DASHBOARD_GENERATED_AT_ID
                        style="color: #475569; margin-top: 0;"
                        data-testid="dashboard-generated-at"
                    >
                        {generated_at}
                    </p>
                </div>
                <DashboardControls />
            </header>

            <section style="display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 0.75rem; margin: 1.25rem 0;">
                <article style="padding: 0.9rem; border: 1px solid #cbd5e1; border-radius: 0.6rem;">
                    <strong id=DASHBOARD_RUNNING_COUNT_ID data-testid="running-count">{running_count}</strong>
                    <div style="color: #64748b;">"running"</div>
                </article>
                <article style="padding: 0.9rem; border: 1px solid #cbd5e1; border-radius: 0.6rem;">
                    <strong id=DASHBOARD_RETRYING_COUNT_ID data-testid="retrying-count">{retrying_count}</strong>
                    <div style="color: #64748b;">"retrying"</div>
                </article>
                <article style="padding: 0.9rem; border: 1px solid #cbd5e1; border-radius: 0.6rem;">
                    <strong id=DASHBOARD_TOTAL_TOKENS_ID data-testid="total-tokens">{total_tokens}</strong>
                    <div style="color: #64748b;">"total tokens"</div>
                </article>
            </section>

            <section style="margin: 1.25rem 0; display: grid; gap: 0.1rem;">
                <p id=DASHBOARD_INPUT_TOKENS_ID style="margin: 0.1rem 0;">{input_tokens}</p>
                <p id=DASHBOARD_OUTPUT_TOKENS_ID style="margin: 0.1rem 0;">{output_tokens}</p>
                <p id=DASHBOARD_RUNTIME_SECONDS_ID style="margin: 0.1rem 0;">{runtime_seconds}</p>
            </section>

            <section style="margin: 1.25rem 0;">
                <h2 style="font-size: 1.1rem;">"Running Sessions"</h2>
                <table style="width: 100%; border-collapse: collapse; border: 1px solid #cbd5e1;">
                    <thead>
                        <tr style="background: #f8fafc; text-align: left;">
                            <th style="padding: 0.5rem; border-bottom: 1px solid #cbd5e1;">"Issue"</th>
                            <th style="padding: 0.5rem; border-bottom: 1px solid #cbd5e1;">"State"</th>
                            <th style="padding: 0.5rem; border-bottom: 1px solid #cbd5e1;">"Session"</th>
                            <th style="padding: 0.5rem; border-bottom: 1px solid #cbd5e1;">"Turns"</th>
                            <th style="padding: 0.5rem; border-bottom: 1px solid #cbd5e1;">"Last Event"</th>
                            <th style="padding: 0.5rem; border-bottom: 1px solid #cbd5e1;">"Tokens"</th>
                        </tr>
                    </thead>
                    <tbody id=DASHBOARD_RUNNING_ROWS_ID inner_html=running_rows_html />
                </table>
            </section>

            <section style="margin: 1.25rem 0;">
                <h2 style="font-size: 1.1rem;">"Retry Queue"</h2>
                <table style="width: 100%; border-collapse: collapse; border: 1px solid #cbd5e1;">
                    <thead>
                        <tr style="background: #f8fafc; text-align: left;">
                            <th style="padding: 0.5rem; border-bottom: 1px solid #cbd5e1;">"Issue"</th>
                            <th style="padding: 0.5rem; border-bottom: 1px solid #cbd5e1;">"Attempt"</th>
                            <th style="padding: 0.5rem; border-bottom: 1px solid #cbd5e1;">"Due At"</th>
                            <th style="padding: 0.5rem; border-bottom: 1px solid #cbd5e1;">"Error"</th>
                        </tr>
                    </thead>
                    <tbody id=DASHBOARD_RETRY_ROWS_ID inner_html=retry_rows_html />
                </table>
            </section>

            <section style="margin: 1.25rem 0;">
                <h2 style="font-size: 1.1rem;">"Rate Limits"</h2>
                <pre
                    id=DASHBOARD_RATE_LIMITS_ID
                    data-testid="dashboard-rate-limits"
                    style="padding: 0.9rem; border: 1px solid #cbd5e1; border-radius: 0.6rem; background: #f8fafc; overflow-x: auto; white-space: pre-wrap;"
                >
                    {rate_limits_text}
                </pre>
            </section>
        </main>
    }
}

fn format_rate_limits(rate_limits: Option<&serde_json::Value>) -> String {
    rate_limits
        .and_then(|value| serde_json::to_string_pretty(value).ok())
        .unwrap_or_else(|| "No rate limit payload received yet".to_string())
}

fn render_generated_at(snapshot: &RuntimeSnapshot) -> String {
    format!("generated at {}", snapshot.generated_at.to_rfc3339())
}

fn format_running_rows_html(rows: &[RunningSnapshotRow]) -> String {
    if rows.is_empty() {
        return [
            r#"<tr><td colspan="6" style="padding: 0.75rem; border-top: 1px solid #e2e8f0; color: #64748b;">"#,
            "No active sessions",
            "</td></tr>",
        ]
        .concat();
    }

    rows.iter().map(format_running_row_html).collect()
}

fn format_running_row_html(row: &RunningSnapshotRow) -> String {
    format!(
        concat!(
            "<tr>",
            r#"<td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{}</td>"#,
            r#"<td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{}</td>"#,
            r#"<td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{}</td>"#,
            r#"<td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{}</td>"#,
            r#"<td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{}</td>"#,
            r#"<td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{}</td>"#,
            "</tr>"
        ),
        escape_html_text(&row.issue_identifier),
        escape_html_text(&row.state),
        escape_html_text(row.session_id.as_deref().unwrap_or("-")),
        row.turn_count,
        escape_html_text(row.last_event.as_deref().unwrap_or("-")),
        row.tokens.total_tokens,
    )
}

fn format_retry_rows_html(rows: &[RetrySnapshotRow]) -> String {
    if rows.is_empty() {
        return [
            r#"<tr><td colspan="4" style="padding: 0.75rem; border-top: 1px solid #e2e8f0; color: #64748b;">"#,
            "No queued retries",
            "</td></tr>",
        ]
        .concat();
    }

    rows.iter().map(format_retry_row_html).collect()
}

fn format_retry_row_html(row: &RetrySnapshotRow) -> String {
    format!(
        concat!(
            "<tr>",
            r#"<td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{}</td>"#,
            r#"<td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{}</td>"#,
            r#"<td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{}</td>"#,
            r#"<td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{}</td>"#,
            "</tr>"
        ),
        escape_html_text(&row.issue_identifier),
        row.attempt,
        escape_html_text(&row.due_at.to_rfc3339()),
        escape_html_text(row.error.as_deref().unwrap_or("-")),
    )
}

fn escape_html_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(feature = "ssr")]
fn escape_json_for_html_script(rendered: &str) -> String {
    rendered
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

#[cfg(feature = "ssr")]
pub(crate) fn dashboard_leptos_options() -> LeptosOptions {
    LeptosOptions::builder()
        .output_name(output_name())
        .site_root(site_root())
        .site_pkg_dir(site_pkg_dir())
        .build()
}

#[cfg(feature = "ssr")]
#[component]
pub(crate) fn DashboardShell(
    options: LeptosOptions,
    escaped_snapshot_json: String,
    snapshot: RuntimeSnapshot,
) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>"Symphony Dashboard"</title>
                <AutoReload options=options.clone() />
                <HydrationScripts options />
                <script
                    id=DASHBOARD_STATE_SCRIPT_ID
                    type="application/json"
                    inner_html=escaped_snapshot_json
                />
            </head>
            <body>
                <DashboardApp snapshot=snapshot />
            </body>
        </html>
    }
}

#[cfg(feature = "ssr")]
pub(crate) fn render_dashboard(snapshot: RuntimeSnapshot) -> impl IntoView {
    let snapshot_json =
        serde_json::to_string(&snapshot).expect("dashboard snapshot should serialize");
    let escaped_snapshot_json = escape_json_for_html_script(&snapshot_json);

    view! {
        <DashboardShell
            options=dashboard_leptos_options()
            escaped_snapshot_json=escaped_snapshot_json
            snapshot=snapshot
        />
    }
}

#[cfg(feature = "ssr")]
pub(crate) fn output_name() -> &'static str {
    match option_env!("LEPTOS_OUTPUT_NAME") {
        Some(value) => value,
        None => "symphony-app",
    }
}

#[cfg(feature = "ssr")]
pub(crate) fn site_pkg_dir() -> &'static str {
    match option_env!("LEPTOS_SITE_PKG_DIR") {
        Some(value) => value,
        None => "pkg",
    }
}

#[cfg(feature = "ssr")]
pub(crate) fn site_root() -> &'static str {
    match option_env!("LEPTOS_SITE_ROOT") {
        Some(value) => value,
        None => "target/site",
    }
}

#[cfg(feature = "hydrate")]
fn browser_document() -> leptos::web_sys::Document {
    leptos::web_sys::window()
        .expect("dashboard hydration requires a browser window")
        .document()
        .expect("dashboard hydration requires a browser document")
}

#[cfg(feature = "hydrate")]
fn dashboard_element_by_id(id: &str) -> leptos::web_sys::Element {
    browser_document()
        .get_element_by_id(id)
        .unwrap_or_else(|| panic!("dashboard element {id} should be present in SSR HTML"))
}

#[cfg(feature = "hydrate")]
fn set_dashboard_text(id: &str, value: &str) {
    dashboard_element_by_id(id).set_text_content(Some(value));
}

#[cfg(feature = "hydrate")]
fn set_dashboard_html(id: &str, value: &str) {
    dashboard_element_by_id(id).set_inner_html(value);
}

#[cfg(feature = "hydrate")]
fn set_refresh_button_disabled(disabled: bool) {
    let button = dashboard_element_by_id(DASHBOARD_REFRESH_ID)
        .dyn_into::<leptos::web_sys::HtmlButtonElement>()
        .expect("dashboard refresh control should be a button element");
    button.set_disabled(disabled);
}

#[cfg(feature = "hydrate")]
fn set_live_status(value: &str) {
    set_dashboard_text(DASHBOARD_LIVE_STATUS_ID, value);
}

#[cfg(feature = "hydrate")]
fn attach_refresh_click_handler() {
    let button = dashboard_element_by_id(DASHBOARD_REFRESH_ID);
    let callback = Closure::wrap(Box::new(move |_event: leptos::web_sys::MouseEvent| {
        spawn_local(async {
            refresh_dashboard_from_server().await;
        });
    }) as Box<dyn FnMut(leptos::web_sys::MouseEvent)>);

    button
        .add_event_listener_with_callback("click", callback.as_ref().unchecked_ref())
        .expect("dashboard refresh click handler should attach");
    callback.forget();
}

#[cfg(feature = "hydrate")]
fn apply_runtime_snapshot_to_dom(snapshot: &RuntimeSnapshot) {
    set_dashboard_text(DASHBOARD_GENERATED_AT_ID, &render_generated_at(snapshot));
    set_dashboard_text(
        DASHBOARD_RUNNING_COUNT_ID,
        &snapshot.counts.running.to_string(),
    );
    set_dashboard_text(
        DASHBOARD_RETRYING_COUNT_ID,
        &snapshot.counts.retrying.to_string(),
    );
    set_dashboard_text(
        DASHBOARD_TOTAL_TOKENS_ID,
        &snapshot.codex_totals.total_tokens.to_string(),
    );
    set_dashboard_text(
        DASHBOARD_INPUT_TOKENS_ID,
        &format!("input: {}", snapshot.codex_totals.input_tokens),
    );
    set_dashboard_text(
        DASHBOARD_OUTPUT_TOKENS_ID,
        &format!("output: {}", snapshot.codex_totals.output_tokens),
    );
    set_dashboard_text(
        DASHBOARD_RUNTIME_SECONDS_ID,
        &format!("runtime seconds: {:.1}", snapshot.codex_totals.seconds_running),
    );
    set_dashboard_html(
        DASHBOARD_RUNNING_ROWS_ID,
        &format_running_rows_html(&snapshot.running),
    );
    set_dashboard_html(
        DASHBOARD_RETRY_ROWS_ID,
        &format_retry_rows_html(&snapshot.retrying),
    );
    set_dashboard_text(
        DASHBOARD_RATE_LIMITS_ID,
        &format_rate_limits(snapshot.rate_limits.as_ref()),
    );
}

#[cfg(feature = "hydrate")]
async fn sync_dashboard_from_server() {
    set_refresh_button_disabled(true);
    set_live_status(LIVE_SYNCING_STATUS);

    match fetch_runtime_snapshot().await {
        Ok(snapshot) => {
            apply_runtime_snapshot_to_dom(&snapshot);
            set_live_status(LIVE_READY_STATUS);
        }
        Err(error) => {
            set_live_status(&format!("Live sync failed: {error}"));
        }
    }

    set_refresh_button_disabled(false);
}

#[cfg(feature = "hydrate")]
async fn refresh_dashboard_from_server() {
    set_refresh_button_disabled(true);
    set_live_status(MANUAL_REFRESHING_STATUS);

    match queue_refresh_and_fetch_snapshot().await {
        Ok(snapshot) => {
            apply_runtime_snapshot_to_dom(&snapshot);
            set_live_status(MANUAL_REFRESHED_STATUS);
        }
        Err(error) => {
            set_live_status(&format!("Refresh failed: {error}"));
        }
    }

    set_refresh_button_disabled(false);
}

#[cfg(feature = "hydrate")]
pub(crate) fn hydrate_dashboard() {
    attach_refresh_click_handler();
    spawn_local(async {
        sync_dashboard_from_server().await;
    });
}

#[cfg(feature = "hydrate")]
async fn fetch_runtime_snapshot() -> Result<RuntimeSnapshot, String> {
    let response = Request::get("/api/v1/state")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.status() != 200 {
        return Err(format!("state endpoint returned {}", response.status()));
    }
    response
        .json::<RuntimeSnapshot>()
        .await
        .map_err(|error| error.to_string())
}

#[cfg(feature = "hydrate")]
async fn queue_refresh_and_fetch_snapshot() -> Result<RuntimeSnapshot, String> {
    let response = Request::post("/api/v1/refresh")
        .header("Content-Type", "application/json")
        .body("{}")
        .map_err(|error| error.to_string())?
        .send()
        .await
        .map_err(|error| error.to_string())?;

    if response.status() != 202 {
        return Err(format!("refresh endpoint returned {}", response.status()));
    }

    fetch_runtime_snapshot().await
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use leptos::prelude::RenderHtml;
    use leptos::reactive::owner::Owner;
    use symphony_core::{CodexTotalsSnapshot, RuntimeCounts, RuntimeSnapshot, TokenSnapshot};

    use super::{
        DASHBOARD_ROOT_ID, DASHBOARD_STATE_SCRIPT_ID, output_name, render_dashboard,
        site_pkg_dir,
    };

    fn sample_snapshot() -> RuntimeSnapshot {
        RuntimeSnapshot {
            generated_at: Utc.with_ymd_and_hms(2026, 3, 5, 12, 0, 0).unwrap(),
            counts: RuntimeCounts {
                running: 1,
                retrying: 1,
            },
            running: vec![symphony_core::RunningSnapshotRow {
                issue_id: "issue-1".to_string(),
                issue_identifier: "MT-101".to_string(),
                state: "In Progress".to_string(),
                session_id: Some("session-1".to_string()),
                turn_count: 2,
                last_event: Some("turn/completed".to_string()),
                last_message: Some("working".to_string()),
                started_at: Utc.with_ymd_and_hms(2026, 3, 5, 11, 55, 0).unwrap(),
                last_event_at: Some(Utc.with_ymd_and_hms(2026, 3, 5, 11, 59, 0).unwrap()),
                tokens: TokenSnapshot {
                    input_tokens: 10,
                    output_tokens: 4,
                    total_tokens: 14,
                },
            }],
            retrying: vec![symphony_core::RetrySnapshotRow {
                issue_id: "issue-2".to_string(),
                issue_identifier: "MT-102".to_string(),
                attempt: 3,
                due_at: Utc.with_ymd_and_hms(2026, 3, 5, 12, 5, 0).unwrap(),
                error: Some("turn_timeout".to_string()),
            }],
            codex_totals: CodexTotalsSnapshot {
                input_tokens: 10,
                output_tokens: 4,
                total_tokens: 14,
                seconds_running: 120.0,
            },
            rate_limits: Some(serde_json::json!({"requests_remaining": 42})),
        }
    }

    #[test]
    fn render_dashboard_embeds_snapshot_and_controls() {
        let html = Owner::new().with(|| render_dashboard(sample_snapshot()).to_html());

        assert!(html.contains("Symphony Runtime"));
        assert!(html.contains(DASHBOARD_ROOT_ID));
        assert!(html.contains(DASHBOARD_STATE_SCRIPT_ID));
        assert!(html.contains("Refresh dashboard"));
        assert!(html.contains("Hydration pending"));
        assert!(html.contains(&format!("/{}/{}.js", site_pkg_dir(), output_name())));
        assert!(html.to_ascii_lowercase().contains("<!doctype html>"));
    }

    #[test]
    fn render_dashboard_escapes_embedded_json_script_content() {
        let mut snapshot = sample_snapshot();
        snapshot.running[0].last_message = Some("</script><script>alert(1)</script>".to_string());

        let html = Owner::new().with(|| render_dashboard(snapshot).to_html());

        assert!(!html.contains("</script><script>alert(1)</script>"));
        assert!(
            html.contains("\\u003c/script\\u003e\\u003cscript\\u003ealert(1)\\u003c/script\\u003e")
        );
    }
}
