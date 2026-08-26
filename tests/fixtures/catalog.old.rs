fn render(&self, data: &Catalog) -> View {
    let page = Page::new();
    match data.get() {
        None => view! { <p class="muted">"Loading…"</p> }.into_any(),
        Some(_) => view! {
            <div class="results">
                <table>
                    <thead>
                        <tr><th>"Name"</th><th>"System"</th></tr>
                    </thead>
                    <tbody>
                        {move || {
                            let rows = rows();
                            rows.into_iter().map(entity_row).collect_view()
                        }}
                    </tbody>
                </table>
            </div>
        }.into_any(),
    }
}
