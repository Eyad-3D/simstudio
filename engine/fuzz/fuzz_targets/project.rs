//! Project files: JSON bytes (an input that starts with `{`), or edits of
//! one of the example projects drawn with `arbitrary`; then one of the
//! project's cases imported, built and run for at most 2 s.
#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use lsim_fuzz::{case_ids, examples, project_case};
use serde_json::Value;

/// A value to put somewhere in a project.
#[derive(Arbitrary, Debug)]
enum New {
    Null,
    Bool(bool),
    Float(f64),
    Int(i64),
    Text(String),
    EmptyList,
    EmptyObject,
    /// the value found at another path of the same project
    Copy(Vec<u16>),
}

/// One edit at a path (each step picks a key or an element, modulo).
#[derive(Arbitrary, Debug)]
enum Edit {
    Set(Vec<u16>, New),
    Remove(Vec<u16>),
    Duplicate(Vec<u16>),
}

#[derive(Arbitrary, Debug)]
struct Edits {
    project: u8,
    case: u8,
    edits: Vec<Edit>,
}

/// A step into an object (its key) or an array (its index).
enum Key {
    Name(String),
    At(usize),
}

/// The keys `path` picks, as far as there are values to go into.
fn keys(mut v: &Value, path: &[u16]) -> Vec<Key> {
    let mut out = vec![];
    for &s in path {
        match v {
            Value::Object(m) if !m.is_empty() => {
                let (k, child) = m.iter().nth(s as usize % m.len()).expect("in range");
                out.push(Key::Name(k.clone()));
                v = child;
            }
            Value::Array(a) if !a.is_empty() => {
                let i = s as usize % a.len();
                out.push(Key::At(i));
                v = &a[i];
            }
            _ => break,
        }
    }
    out
}

fn child<'a>(v: &'a mut Value, k: &Key) -> &'a mut Value {
    match (v, k) {
        (Value::Object(m), Key::Name(n)) => m.get_mut(n).expect("a key keys() found"),
        (Value::Array(a), Key::At(i)) => &mut a[*i],
        _ => unreachable!("keys() follows the value's shape"),
    }
}

fn apply(p: &mut Value, e: &Edit) {
    let path = match e {
        Edit::Set(path, _) | Edit::Remove(path) | Edit::Duplicate(path) => path,
    };
    let ks = keys(p, path);
    let value = match e {
        Edit::Set(_, New::Copy(from)) => {
            Some(keys(p, from).iter().fold(&mut p.clone(), |v, k| child(v, k)).clone())
        }
        Edit::Set(_, new) => Some(match new {
            New::Null => Value::Null,
            New::Bool(b) => Value::Bool(*b),
            New::Float(x) => serde_json::Number::from_f64(*x).map_or(Value::Null, Value::Number),
            New::Int(i) => Value::from(*i),
            New::Text(s) => Value::String(s.clone()),
            New::EmptyList => Value::Array(vec![]),
            New::EmptyObject => Value::Object(Default::default()),
            New::Copy(_) => unreachable!(),
        }),
        _ => None,
    };
    let Some((last, parents)) = ks.split_last() else { return };
    let parent = parents.iter().fold(p, |v, k| child(v, k));
    match (e, parent, last) {
        (Edit::Set(..), v, k) => *child(v, k) = value.expect("a Set has a value"),
        (Edit::Remove(_), Value::Object(m), Key::Name(n)) => {
            m.remove(n);
        }
        (Edit::Remove(_), Value::Array(a), Key::At(i)) => {
            a.remove(*i);
        }
        (Edit::Duplicate(_), Value::Array(a), Key::At(i)) => {
            let x = a[*i].clone();
            a.insert(*i, x);
        }
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    let (project, pick) = if data.first() == Some(&b'{') {
        match serde_json::from_slice::<Value>(data) {
            Ok(p) => (p, data.len()),
            Err(_) => return,
        }
    } else {
        let Ok(e) = Edits::arbitrary(&mut Unstructured::new(data)) else { return };
        let all = examples();
        let mut p = all[e.project as usize % all.len()].1.clone();
        for x in e.edits.iter().take(16) {
            apply(&mut p, x);
        }
        (p, e.case as usize)
    };
    let ids = case_ids(&project);
    let case = (!ids.is_empty()).then(|| ids[pick % ids.len()].as_str());
    project_case(&project, case);
});
