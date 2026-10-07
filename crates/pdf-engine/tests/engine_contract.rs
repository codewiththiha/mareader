/
/
!

W
i
r
e
-
c
o
n
t
r
a
c
t

g
u
a
r
d

f
o
r

t
h
e

`
w
i
n
d
o
w
.
P
D
F
R
e
a
d
e
r
`

f
a
c
a
d
e
.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repo root
    p
}

/// Extern names declared under `js_namespace = ["window", "PDFReader"]`.
fn bridge_pdfreader_names() -> Vec<String> {
    let bridge = std::fs::read_to_string(repo_root().join("crates/pdf-engine/src/bridge.rs"))
        .expect("bridge.rs must exist next to the test");
    let mut names = Vec::new();
    /
    /

    T
    h
    e

    l
    a
    s
    t

    `
    #
    [
    w
    a
    s
    m
    _
    b
    i
    n
    d
    g
    e
    n
    (
    .
    .
    .
    )
    ]
    `

    a
    t
    t
    r
    i
    b
    u
    t
    e

    s
    e
    e
    n
    .
    let mut attr: Option<(bool, Option<String>)> = None; // (pdfreader ns, js_name)
    for line in bridge.lines() {
        if line.contains("#[wasm_bindgen(") {
            let pdfreader = line.contains("js_namespace = [\"window\", \"PDFReader\"]");
            /
            /

            M
            a
            t
            c
            h

            `
            j
            s
            _
            n
            a
            m
            e

            =

            "
            X
            "
            `

            o
            n
            l
            y
            :

            `
            j
            s
            _
            n
            a
            m
            e
            s
            p
            a
            c
            e
            `

            c
            o
            n
            t
            a
            i
            n
            s

            t
            h
            a
            t

            s
            u
            b
            s
            t
            r
            i
            n
            g
            .
            let js_name = line
                .split("js_name = \"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .map(str::to_string);
            attr = Some((pdfreader, js_name));
            continue;
        }
        if let Some(start) = line.find("pub ") {
            let rest = &line[start + 4..];
            if let Some(fn_pos) = rest.find("fn ") {
                let name: String = rest[fn_pos + 3..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if let Some((true, js_name)) = attr.take() {
                    names.push(js_name.unwrap_or(name));
                }
                continue;
            }
        }
    }
    names
}

/
/
/

T
r
u
e

w
h
e
n

`
n
a
m
e
`

a
p
p
e
a
r
s

i
n

t
h
e

f
a
c
a
d
e

a
s

a

p
r
o
p
e
r
t
y

k
e
y
.
fn facade_has_key(facade: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let hay: Vec<char> = facade.chars().collect();
    let needle: Vec<char> = name.chars().collect();
    for (i, window) in hay.windows(needle.len()).enumerate() {
        if window != needle.as_slice() {
            continue;
        }
        let before_ok = i == 0 || !(hay[i - 1].is_alphanumeric() || hay[i - 1] == '_');
        if !before_ok {
            continue;
        }
        let mut j = i + needle.len();
        while j < hay.len() && hay[j].is_whitespace() {
            j += 1;
        }
        if j < hay.len() && (hay[j] == ',' || hay[j] == ':' || hay[j] == '}') {
            return true;
        }
    }
    false
}

#[test]
fn engine_facade_exposes_every_pdfreader_binding() {
    let facade = std::fs::read_to_string(repo_root().join("public/pdfEngine.js"));
    let Ok(facade) = facade else {
        eprintln!(
            "public/pdfEngine.js not built — facade contract check skipped \
             (run `npm run build:ts`)"
        );
        return;
    };
    let names = bridge_pdfreader_names();
    assert!(
        !names.is_empty(),
        "no PDFReader externs parsed from bridge.rs"
    );
    let missing: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| !facade_has_key(&facade, name))
        .collect();
    assert!(
        missing.is_empty(),
        "public/pdfEngine.js is missing bridge bindings: {}",
        missing.join(", ")
    );
}
