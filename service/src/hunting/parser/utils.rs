use nom::bytes::complete::{tag, take_while1};
use nom::character::complete::{i64, multispace0, u64};
use nom::combinator::{map, opt, consumed};
use nom::error::ParseError;
use nom::sequence::{delimited, pair, preceded};
use nom::{IResult, InputLength, Parser, InputTake, InputIter, InputTakeAtPosition, AsChar};

type Decimal = (i64, Option<(usize, u64)>);

pub fn dec_to_i64(dec: Decimal, precision: u64) -> i64 {
    let fracional = dec.1.unwrap_or((0, 0));
    dec.0 * precision as i64 + (fracional.1 * precision / 10_u64.pow(fracional.0 as u32)) as i64
}

#[inline]
pub fn is_kql_identifier(chr: char) -> bool {
    chr.is_alphanumeric() || chr == '_'
}

#[inline]
pub fn is_kql_wildcard_identifier(chr: char) -> bool {
    is_kql_identifier(chr) || chr == '*'
}

pub fn take_identifier(i: &str) -> IResult<&str, &str> {
    let (input, identifier) = take_while1(is_kql_identifier)(i)?;

    // exclude reserved keywords
    if identifier == "by" {
        return Err(nom::Err::Error(nom::error::Error::new(i, nom::error::ErrorKind::Tag)));
    }
    Ok((input, identifier))
}

pub fn decimal_number(i: &str) -> IResult<&str, Decimal> {
    pair(i64, opt(preceded(tag("."), map(consumed(u64::<&str, _>), |(i, x)| (i.len(), x)))))(i)
}

pub fn trim<I, O, E, F>(f: F) -> impl FnMut(I) -> IResult<I, O, E>
where
    I: Clone + InputLength + InputTake + InputIter,
    I: InputTakeAtPosition,
    <I as InputTakeAtPosition>::Item: AsChar + Clone,
    F: Parser<I, O, E>,
    E: ParseError<I>,
{
    delimited(multispace0, f, multispace0)
}
