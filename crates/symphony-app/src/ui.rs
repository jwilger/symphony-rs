use leptos::prelude::*;

#[derive(Clone)]
pub(crate) struct DashboardModel {
    pub(crate) generated_at: String,
    pub(crate) running_count: usize,
    pub(crate) retrying_count: usize,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) total_tokens: u64,
    pub(crate) seconds_running: f64,
    pub(crate) rows: Vec<DashboardRow>,
}

#[derive(Clone)]
pub(crate) struct DashboardRow {
    pub(crate) issue_identifier: String,
    pub(crate) state: String,
    pub(crate) session_id: String,
    pub(crate) turn_count: u32,
    pub(crate) last_event: String,
    pub(crate) total_tokens: u64,
}

#[component]
fn Dashboard(model: DashboardModel) -> impl IntoView {
    let rows = model.rows;

    view! {
        <main style="font-family: ui-sans-serif, system-ui, sans-serif; max-width: 1100px; margin: 2rem auto; padding: 0 1rem;">
            <h1 style="font-size: 2rem; margin-bottom: 0.5rem;">"Symphony Runtime"</h1>
            <p style="color: #475569; margin-top: 0;">{format!("generated at {}", model.generated_at)}</p>

            <section style="display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 0.75rem; margin: 1.25rem 0;">
                <article style="padding: 0.9rem; border: 1px solid #cbd5e1; border-radius: 0.6rem;">
                    <strong>{model.running_count}</strong>
                    <div style="color: #64748b;">"running"</div>
                </article>
                <article style="padding: 0.9rem; border: 1px solid #cbd5e1; border-radius: 0.6rem;">
                    <strong>{model.retrying_count}</strong>
                    <div style="color: #64748b;">"retrying"</div>
                </article>
                <article style="padding: 0.9rem; border: 1px solid #cbd5e1; border-radius: 0.6rem;">
                    <strong>{model.total_tokens}</strong>
                    <div style="color: #64748b;">"total tokens"</div>
                </article>
            </section>

            <section style="margin: 1.25rem 0;">
                <p style="margin: 0.1rem 0;">{format!("input: {}", model.input_tokens)}</p>
                <p style="margin: 0.1rem 0;">{format!("output: {}", model.output_tokens)}</p>
                <p style="margin: 0.1rem 0;">{format!("runtime seconds: {:.1}", model.seconds_running)}</p>
            </section>

            <section>
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
                    <tbody>
                        {rows
                            .into_iter()
                            .map(|row| {
                                view! {
                                    <tr>
                                        <td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{row.issue_identifier}</td>
                                        <td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{row.state}</td>
                                        <td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{row.session_id}</td>
                                        <td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{row.turn_count}</td>
                                        <td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{row.last_event}</td>
                                        <td style="padding: 0.5rem; border-top: 1px solid #e2e8f0;">{row.total_tokens}</td>
                                    </tr>
                                }
                            })
                            .collect::<Vec<_>>()}
                    </tbody>
                </table>
            </section>
        </main>
    }
}

pub(crate) fn render_dashboard(model: DashboardModel) -> String {
    let rendered = view! {
        <Dashboard model=model />
    };
    let app_markup = rendered.to_html();

    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Symphony Dashboard</title></head><body>{app_markup}<script type=\"module\" src=\"/pkg/symphony-app.js\"></script></body></html>"
    )
}
