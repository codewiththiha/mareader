use super::*;

#[test]
fn a_picture_takes_its_first_fitting_source() {
    let mut shelf = Shelf::default();
    for (name, w) in [
        ("small.png", 4),
        ("big.webp", 40),
        ("wide.png", 20),
        ("x2.png", 30),
    ] {
        if let Some(p) = png(w, 4) {
            shelf.pictures.insert(format!("/OEBPS/images/{name}"), p);
        }
    }
    let b = read_with(
        &mut shelf,
        "<picture><source srcset='../images/new.avif' type='image/avif'/>\
             <source srcset='../images/wide.png' media='print'/>\
             <source srcset='../images/small.png 1x, ../images/big.webp 2x' type='image/webp'/>\
             <img src='../images/small.png' alt=''/></picture>\
             <picture><img src='../images/small.png' alt=''/></picture>\
             <img srcset='../images/wide.png 300w, ../images/x2.png 900w' alt='no src'/>",
    );
    let sizes: Vec<_> = pictures_of(&b).iter().map(|p| p.width).collect();
    assert_eq!(sizes, [40, 4, 30]);
}

#[test]
fn linked_pictures_and_image_maps_lead_where_they_say() {
    let mut shelf = Shelf::default();
    if let Some(p) = png(200, 100) {
        shelf.pictures.insert("/OEBPS/images/a.png".into(), p);
    }
    let (b, refs) = read_all(
        &mut shelf,
        "<div><a href='c2.xhtml'><img src='../images/a.png' alt=''/></a></div>\
             <img src='../images/a.png' usemap='#nav' alt=''/>\
             <map name='nav'><area shape='rect' coords='0,0,100,50' href='c3.xhtml#top'/>\
             <area shape='circle' coords='150,50,50' href='https://example.org/'/></map>",
    );
    let c2 = Href::Inside("/OEBPS/text/c2.xhtml".into());
    let c3 = Href::Inside("/OEBPS/text/c3.xhtml#top".into());
    let web = Href::Outside("https://example.org/".into());
    let whole = [0.0, 0.0, 1.0, 1.0];
    let want = vec![
        vec![(&c2, whole)],
        vec![(&c3, [0.0, 0.0, 0.5, 0.5]), (&web, [0.5, 0.0, 0.5, 1.0])],
    ];
    assert_eq!(picture_links(&b, &refs), want);
}

#[test]
fn an_iframe_is_a_link_to_its_page() {
    let (b, refs) = read_all(
        &mut Shelf::default(),
        "<iframe src='quiz.xhtml' title='Quiz'>zqx</iframe>",
    );
    assert_eq!(texts(&b), ["Open the embedded page: Quiz"]);
    let quiz = Href::Inside("/OEBPS/text/quiz.xhtml".into());
    assert!(refs.hrefs.contains(&quiz), "{:?}", refs.hrefs);
}
