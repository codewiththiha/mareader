//! Opening a book: the file it lives in, and the page it was left on.

use mareader_library::BookId;
use mareader_session::{Anchor, Source, SpinePlace};

use super::{State, Status};
use crate::documents::OpenRequest;
use crate::toast::Toast;

impl State {
    /// Fills in where an open file was left; held while loading.
    pub fn resume(&mut self, mut request: OpenRequest) -> Option<OpenRequest> {
        let Source::Path(path) = &request.source else {
            return Some(request);
        };
        // Opened as a book, or with a place already: nothing to fill in.
        if request.page != 0
            || request.place.is_some()
            || request.spine.is_some()
            || request.pages > 0
            || request.book.is_some()
        {
            return Some(request);
        }
        if self.status == Status::Loading {
            self.waiting.push(request);
            return None;
        }
        // Recorded as `Documents::open` spells it: canonical.
        let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        let at = self.model.book_at(&real);
        if let Some(p) = at.and_then(|id| self.model.position(id)) {
            request.page = p.page;
            request.place = p.place.map(|(block, offset)| Anchor { block, offset });
            request.spine = p.spine.map(|place| SpinePlace {
                href: place.href,
                item: place.item,
                block: place.block,
                offset: place.offset,
            });
            request.pages = p.pages;
        }
        Some(request)
    }

    /// Where `id`'s file opens, on its page; a toast if it's gone.
    pub fn open(&mut self, id: BookId) -> Result<OpenRequest, Toast> {
        let book = self
            .model
            .book(id)
            .ok_or_else(|| Toast::error("That book was removed."))?;
        let Some(path) = book.path() else {
            let title = &book.title.value;
            return Err(Toast::error(format!("“{title}” isn't where it was.")));
        };
        let position = self.model.position(id);
        let (page, place, spine, pages) = match position {
            Some(position) => (
                position.page,
                position
                    .place
                    .map(|(block, offset)| Anchor { block, offset }),
                position.spine.map(|place| SpinePlace {
                    href: place.href,
                    item: place.item,
                    block: place.block,
                    offset: place.offset,
                }),
                position.pages,
            ),
            None => (0, None, None, 0),
        };
        // Read as this book: a twin keeps its own place.
        Ok(OpenRequest {
            book: Some(id),
            page,
            place,
            spine,
            pages,
            ..OpenRequest::path(path.to_path_buf())
        })
    }
}
