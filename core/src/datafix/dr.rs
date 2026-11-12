// com.mojang.serialization.DataResult: a success, or an error that may hold a
// partial result. The fixer only logs the messages of an error, so they are
// not kept; what an error leaves (its partial result) is.

#[derive(Clone, Debug)]
pub enum Dr<T> {
    Ok(T),
    Err(Option<T>),
}

impl<T> Dr<T> {
    pub fn err() -> Dr<T> {
        Dr::Err(None)
    }

    // DataResult.error(message, partial).
    pub fn partial(value: T) -> Dr<T> {
        Dr::Err(Some(value))
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Dr<U> {
        match self {
            Dr::Ok(v) => Dr::Ok(f(v)),
            Dr::Err(p) => Dr::Err(p.map(f)),
        }
    }

    // Error.flatMap runs the function on the partial result, and the
    // outcome is an error either way.
    pub fn flat_map<U>(self, f: impl FnOnce(T) -> Dr<U>) -> Dr<U> {
        match self {
            Dr::Ok(v) => f(v),
            Dr::Err(None) => Dr::Err(None),
            Dr::Err(Some(p)) => match f(p) {
                Dr::Ok(u) => Dr::Err(Some(u)),
                Dr::Err(q) => Dr::Err(q),
            },
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Dr::Ok(_))
    }

    pub fn has_result_or_partial(&self) -> bool {
        !matches!(self, Dr::Err(None))
    }

    pub fn result(self) -> Option<T> {
        match self {
            Dr::Ok(v) => Some(v),
            Dr::Err(_) => None,
        }
    }

    pub fn result_or_partial(self) -> Option<T> {
        match self {
            Dr::Ok(v) => Some(v),
            Dr::Err(p) => p,
        }
    }

    pub fn set_partial(self, partial: T) -> Dr<T> {
        match self {
            Dr::Ok(v) => Dr::Ok(v),
            Dr::Err(_) => Dr::Err(Some(partial)),
        }
    }

    pub fn as_ref(&self) -> Dr<&T> {
        match self {
            Dr::Ok(v) => Dr::Ok(v),
            Dr::Err(p) => Dr::Err(p.as_ref()),
        }
    }
}

// Applicative ap2 (apply2stable): a success when both are; otherwise an error
// whose partial result is the function of both partial results, when both
// have one.
pub fn ap2<A, B, R>(a: Dr<A>, b: Dr<B>, f: impl FnOnce(A, B) -> R) -> Dr<R> {
    match (a, b) {
        (Dr::Ok(x), Dr::Ok(y)) => Dr::Ok(f(x, y)),
        (a, b) => match (a.result_or_partial(), b.result_or_partial()) {
            (Some(x), Some(y)) => Dr::Err(Some(f(x, y))),
            _ => Dr::Err(None),
        },
    }
}
