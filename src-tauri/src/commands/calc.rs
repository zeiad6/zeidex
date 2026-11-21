use zeidex_calc::Calculation;

/// The inline calculator. `None` means "this query is not arithmetic", so the
/// UI leaves the search results alone. Pure and cheap enough to run on every
/// keystroke, with no debounce.
#[tauri::command]
pub fn calc_eval(query: String) -> Option<Calculation> {
    zeidex_calc::eval_query(&query)
}
