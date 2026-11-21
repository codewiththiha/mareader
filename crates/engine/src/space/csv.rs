//! Space's rows as a CSV file, one quoted line per row shown.

use super::{View, status_label};
use crate::export::{date, one_line};

/// The file name an export offers.
pub const CSV_NAME: &str = "mareader-space.csv";

const HEADER: &str = "lemma,pos,level,meaning,context,book,status,date";

/// The rows `view` shows, in order, every cell quoted.
///
/// A row met in several books names them joined by `; `; the date is the
/// day it was last met, in UTC.
pub fn csv(view: &View) -> String {
    let mut out = format!("{HEADER}\n");
    for row in view.rows() {
        let cells = [
            row.lemma.clone(),
            row.pos.clone().unwrap_or_default(),
            mareader_cefr::label(row.band).to_owned(),
            one_line(&row.meaning),
            one_line(&row.context),
            row.books.join("; "),
            status_label(row.status).to_lowercase(),
            date(row.seen),
        ];
        out += &cells.map(|c| quote(&c)).join(",");
        out.push('\n');
    }
    out
}

pub(super) fn quote(cell: &str) -> String {
    format!("\"{}\"", cell.replace('"', "\"\""))
}
