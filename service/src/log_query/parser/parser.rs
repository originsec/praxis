// nom 8 deprecates `tuple((..))` in favour of bare tuples implementing Parser.
// The rewrites are mechanical but touch every combinator chain in this file;
// suppress the deprecation for now and migrate in a follow-up.
#![allow(deprecated)]

use std::str;

use nom::branch::alt;
use nom::bytes::complete::{tag, tag_no_case, take_while1, escaped, is_a};
use nom::character::complete::{digit1, i32, i64, multispace0, multispace1, none_of, one_of, u32, u64, hex_digit1};
use nom::combinator::{map, opt, recognize, value};
use nom::multi::{many0, separated_list0, separated_list1, fold_many0, many1};
use nom::sequence::{tuple, preceded, delimited, separated_pair, terminated, pair};
use nom::{IResult, Parser};

use super::ast::*;
use super::datetime::{iso8601_datetime, rfc822_datetime, rfc850_datetime};
use super::{dec_to_i64, decimal_number, is_kql_wildcard_identifier, take_identifier, trim};

fn type_tag(i: &str) -> IResult<&str, Type> {
    alt((
        map(tag("bool"), |_| Type::Bool),
        value(Type::DateTime, alt((tag("datetime"), tag("date")))),
        value(Type::Decimal, tag("decimal")),
        value(Type::Dynamic, tag("dynamic")),
        map(tag("int"), |_| Type::Int),
        map(tag("long"), |_| Type::Long),
        value(Type::Real, tag("real")),
        map(tag("string"), |_| Type::String),
        map(tag("timespan"), |_| Type::Timespan),
    )).parse(i)
}

fn option_literal(i: &str) -> IResult<&str, OptionLiteral> {
    alt((
        value(OptionLiteral::Bool(true), tag("true")),
        value(OptionLiteral::Bool(false), tag("false")),
        map(i64, |x| OptionLiteral::Long(x)),
        map(take_while1(|c: char| !c.is_whitespace()), |s: &str| OptionLiteral::String(s.to_string())),
    )).parse(i)
}

fn option_quoted_literal(i: &str) -> IResult<&str, OptionLiteral> {
    alt((
        value(OptionLiteral::Bool(true), tag("true")),
        value(OptionLiteral::Bool(false), tag("false")),
        map(i64, |x| OptionLiteral::Long(x)),
        map(string, |s| OptionLiteral::String(s)),
        map(identifier, |s| OptionLiteral::Identifier(s))
    )).parse(i)
}

fn options(i: &str) -> IResult<&str, Options> {
    map(separated_list0(multispace1, separated_pair(
        identifier,
        trim(tag("=")),
        option_literal
    )), |x| x.into_iter().collect()).parse(i)
}

fn options_with_comma_and_quoted(i: &str) -> IResult<&str, Options> {
    map(separated_list0(tag(","), separated_pair(
        trim(identifier),
        tag("="),
        trim(option_quoted_literal)
    )), |x| x.into_iter().collect()).parse(i)
}

fn pattern(i: &str) -> IResult<&str, Vec<PatternToken>> {
    many1(trim(alt((
        map(tag("*"), |_| PatternToken::Wildcard),
        map(string, |s| PatternToken::String(s)),
        map(
            pair(identifier, opt(preceded(trim(tag(":")), type_tag))),
            |(n, t)| PatternToken::Column(n, t)
        )
    )))).parse(i)
}

fn type_mapping(i: &str) -> IResult<&str, Vec<(String, Type)>> {
    separated_list1(tag(","), separated_pair(
        trim(identifier),
        tag(":"),
        trim(type_tag)
    )).parse(i)
}

fn identifier(i: &str) -> IResult<&str, String> {
    map(take_identifier, |i| i.to_string()).parse(i)
}

fn wildcard_identifier(i: &str) -> IResult<&str, String> {
    map(take_while1(is_kql_wildcard_identifier), |i: &str| i.to_string()).parse(i)
}

fn string(i: &str) -> IResult<&str, String> {
    map(alt((
        delimited(tag("\""), alt((escaped(none_of::<&str, _, _>("\\\""), '\\', tag("\"")), tag(""))), tag("\"")),
        delimited(tag("'"), alt((escaped(none_of::<&str, _, _>("\\'"), '\\', tag("'")), tag(""))), tag("'"))
    )), |s| s.to_string()).parse(i)
}

fn boolean(i: &str) -> IResult<&str, Option<bool>> {
    alt((
        map(tag_no_case("true"), |_| Some(true)),
        map(tag_no_case("false"), |_| Some(false)),
        map(i64, |x| Some(x != 0)),
        map(tag("null"), |_| None)
    )).parse(i)
}

fn date(i: &str) -> IResult<&str, Option<DateTime>> {
    alt((
        map(iso8601_datetime, |x| Some(x)),
        map(rfc822_datetime, |x| Some(x)),
        map(rfc850_datetime, |x| Some(x)),
        map(tag("null"), |_| None)
    )).parse(i)
}

fn decimal(i: &str) -> IResult<&str, Option<f64>> {
    alt((
        map(recognize(tuple((opt(tag("-")), digit1, opt(pair(tag("."), digit1)), opt(tuple((tag("e"), opt(tag("-")), digit1)))))), |x: &str| Some(x.parse().unwrap())),
        value(Some(f64::INFINITY), tag("+inf")),
        value(Some(f64::INFINITY), tag("-inf")),
        value(Some(f64::NAN), tag("nan")),
        value(None, tag("null")),
    )).parse(i)
}

fn dynamic(i: &str) -> IResult<&str, Option<Dynamic>> {
    alt((
        map(delimited(tag("["), separated_list0(tag(","), trim(dynamic)), tag("]")), |x| Some(Dynamic::Array(x))),
        map(delimited(tag("{"), separated_list0(tag(","), separated_pair(trim(string), tag(":"), trim(dynamic))), tag("}")), |x| Some(Dynamic::Dictionary(x.into_iter().collect()))),
        map(preceded(tag("bool"), delimited(tag("("), trim(boolean), tag(")"))), |x| Some(Dynamic::Bool(x))),
        map(preceded(tag("datetime"), delimited(tag("("), trim(date), tag(")"))), |x| Some(Dynamic::DateTime(x))),
        map(preceded(tag("decimal"), delimited(tag("("), trim(decimal), tag(")"))), |x| Some(Dynamic::Decimal(x))),
        map(preceded(tag("int"), delimited(tag("("), trim(integer), tag(")"))), |x| Some(Dynamic::Int(x))),
        map(preceded(tag("long"), delimited(tag("("), trim(long), tag(")"))), |x| Some(Dynamic::Long(x))),
        map(preceded(alt((tag("timespan"), tag("time"))), delimited(tag("("), trim(timespan), tag(")"))), |x| Some(Dynamic::Timespan(x))),
        map(preceded(tag_no_case("0x"), hex_digit1), |x| Some(Dynamic::Long(Some(i64::from_str_radix(x, 16).unwrap())))),
        map(terminated(decimal_number, alt((tag("days"), tag("day"), tag("d")))), |x| Some(Dynamic::Timespan(Some(dec_to_i64(x, 1000 * 1000 * 1000 * 60 * 60 * 24))))),
        map(terminated(decimal_number, alt((tag("hours"), tag("hour"), tag("h")))), |x| Some(Dynamic::Timespan(Some(dec_to_i64(x, 1000 * 1000 * 1000 * 60 * 60))))),
        map(terminated(decimal_number, alt((tag("minutes"), tag("minute"), tag("m")))), |x| Some(Dynamic::Timespan(Some(dec_to_i64(x, 1000 * 1000 * 1000 * 60))))),
        map(terminated(decimal_number, alt((tag("seconds"), tag("second"), tag("s")))), |x| Some(Dynamic::Timespan(Some(dec_to_i64(x, 1000 * 1000 * 1000))))),
        map(terminated(decimal_number, alt((tag("milliseconds"), tag("millisecond"), tag("milli"), tag("ms")))), |x| Some(Dynamic::Timespan(Some(dec_to_i64(x, 1000 * 1000))))),
        map(terminated(decimal_number, alt((tag("microseconds"), tag("microsecond"), tag("micro")))), |x| Some(Dynamic::Timespan(Some(dec_to_i64(x, 1000))))),
        map(terminated(decimal_number, alt((tag("ticks"), tag("tick")))), |x| Some(Dynamic::Timespan(Some(dec_to_i64(x, 100))))),
        map(recognize(tuple((opt(tag("-")), digit1, tag("."), digit1, opt(tuple((tag("e"), opt(tag("-")), digit1)))))), |x: &str| Some(Dynamic::Real(Some(x.parse().unwrap())))),
        map(recognize(tuple((opt(tag("-")), digit1, tag("e"), opt(tag("-")), digit1))), |x: &str| Some(Dynamic::Real(Some(x.parse().unwrap())))),
        map(i64, |x| Some(Dynamic::Long(Some(x)))),
        map(string, |s| Some(Dynamic::String(s))),
        alt((
            value(Some(Dynamic::Bool(Some(true))), tag("true")),
            value(Some(Dynamic::Bool(Some(false))), tag("false")),
            value(None, tag("null"))
        ))
    )).parse(i)
}

fn integer(i: &str) -> IResult<&str, Option<i32>> {
    alt((
        map(preceded(tag_no_case("0x"), hex_digit1), |x| Some(i32::from_str_radix(x, 16).unwrap())),
        map(i32, |x| Some(x)),
        map(tag("null"), |_| None)
    )).parse(i)
}

fn long(i: &str) -> IResult<&str, Option<i64>> {
    alt((
        map(preceded(tag_no_case("0x"), hex_digit1), |x| Some(i64::from_str_radix(x, 16).unwrap())),
        map(i64, |x| Some(x)),
        map(tag("null"), |_| None)
    )).parse(i)
}

fn real(i: &str) -> IResult<&str, Option<f32>> {
    alt((
        map(recognize(tuple((opt(tag("-")), digit1, opt(pair(tag("."), digit1)), opt(tuple((tag("e"), opt(tag("-")), digit1)))))), |x: &str| Some(x.parse().unwrap())),
        value(Some(f32::INFINITY), tag("+inf")),
        value(Some(f32::INFINITY), tag("-inf")),
        value(Some(f32::NAN), tag("nan")),
        value(None, tag("null")),
    )).parse(i)
}

fn timespan(i: &str) -> IResult<&str, Option<i64>> {
    alt((
        map(terminated(decimal_number, pair(multispace0, alt((tag("days"), tag("day"), tag("d"))))), |x| Some(dec_to_i64(x, 1000 * 1000 * 1000 * 60 * 60 * 24))),
        map(terminated(decimal_number, pair(multispace0, alt((tag("hours"), tag("hour"), tag("h"))))), |x| Some(dec_to_i64(x, 1000 * 1000 * 1000 * 60 * 60))),
        map(terminated(decimal_number, pair(multispace0, alt((tag("minutes"), tag("minute"), tag("m"))))), |x| Some(dec_to_i64(x, 1000 * 1000 * 1000 * 60))),
        map(terminated(decimal_number, pair(multispace0, alt((tag("seconds"), tag("second"), tag("s"))))), |x| Some(dec_to_i64(x, 1000 * 1000 * 1000))),
        map(terminated(decimal_number, pair(multispace0, alt((tag("milliseconds"), tag("millisecond"), tag("milli"), tag("ms"))))), |x| Some(dec_to_i64(x, 1000 * 1000))),
        map(terminated(decimal_number, pair(multispace0, alt((tag("microseconds"), tag("microsecond"), tag("micro"))))), |x| Some(dec_to_i64(x, 1000))),
        map(terminated(decimal_number, pair(multispace0, alt((tag("ticks"), tag("tick"))))), |x| Some(dec_to_i64(x, 100))),
        map(
            tuple((separated_pair(i64, tag("."), separated_pair(u64, tag(":"), u64)), opt(preceded(tag(":"), decimal_number)))),
            |((d, (h, m)), s)| Some(((d * 24 + h as i64) * 60 + m as i64) * (1000 * 1000 * 1000 * 60) + s.map(|x| dec_to_i64(x, 1000 * 1000 * 1000)).unwrap_or(0) as i64)
        ),
        map(
            tuple((separated_pair(u64, tag(":"), u64), opt(preceded(tag(":"), decimal_number)))),
            |((h, m), s)| Some((h as i64 * 60 + m as i64) * (1000 * 1000 * 1000 * 60) + s.map(|x| dec_to_i64(x, 1000 * 1000 * 1000)).unwrap_or(0) as i64)
        ),
        map(tag("null"), |_| None)
    )).parse(i)
}

fn literal(i: &str) -> IResult<&str, Literal> {
    alt((
        map(preceded(tag("bool"), delimited(tag("("), trim(boolean), tag(")"))), |x| Literal::Bool(x)),
        map(preceded(tag("datetime"), delimited(tag("("), trim(date), tag(")"))), |x| Literal::DateTime(x)),
        map(preceded(tag("dynamic"), delimited(tag("("), trim(dynamic), tag(")"))), |x| Literal::Dynamic(x)),
        map(preceded(tag("int"), delimited(tag("("), trim(integer), tag(")"))), |x| Literal::Int(x)),
        map(preceded(tag("long"), delimited(tag("("), trim(long), tag(")"))), |x| Literal::Long(x)),
        map(preceded(tag("real"), delimited(tag("("), trim(real), tag(")"))), |x| Literal::Real(x)),
        map(preceded(alt((tag("timespan"), tag("time"))), delimited(tag("("), trim(timespan), tag(")"))), |x| Literal::Timespan(x)),
        map(preceded(tag_no_case("0x"), hex_digit1), |x| Literal::Long(Some(i64::from_str_radix(x, 16).unwrap()))),
        map(terminated(decimal_number, alt((tag("days"), tag("day"), tag("d")))), |x| Literal::Timespan(Some(dec_to_i64(x, 1000 * 1000 * 1000 * 60 * 60 * 24)))),
        map(terminated(decimal_number, alt((tag("hours"), tag("hour"), tag("h")))), |x| Literal::Timespan(Some(dec_to_i64(x, 1000 * 1000 * 1000 * 60 * 60)))),
        map(terminated(decimal_number, alt((tag("minutes"), tag("minute"), tag("m")))), |x| Literal::Timespan(Some(dec_to_i64(x, 1000 * 1000 * 1000 * 60)))),
        map(terminated(decimal_number, alt((tag("seconds"), tag("second"), tag("s")))), |x| Literal::Timespan(Some(dec_to_i64(x, 1000 * 1000 * 1000)))),
        map(terminated(decimal_number, alt((tag("milliseconds"), tag("millisecond"), tag("milli"), tag("ms")))), |x| Literal::Timespan(Some(dec_to_i64(x, 1000 * 1000)))),
        map(terminated(decimal_number, alt((tag("microseconds"), tag("microsecond"), tag("micro")))), |x| Literal::Timespan(Some(dec_to_i64(x, 1000)))),
        map(terminated(decimal_number, alt((tag("ticks"), tag("tick")))), |x| Literal::Timespan(Some(dec_to_i64(x, 100)))),
        map(recognize(tuple((opt(tag("-")), digit1, tag("."), digit1, opt(tuple((tag("e"), opt(tag("-")), digit1)))))), |x: &str| Literal::Real(Some(x.parse().unwrap()))),
        map(recognize(tuple((opt(tag("-")), digit1, tag("e"), opt(tag("-")), digit1))), |x: &str| Literal::Real(Some(x.parse().unwrap()))),
        map(i64, |x| Literal::Long(Some(x))),
        map(string, |s| Literal::String(s)),
        map(tag("true"), |_| Literal::Bool(Some(true))),
        map(tag("false"), |_| Literal::Bool(Some(false))),
    )).parse(i)
}

fn ident_expr(i: &str) -> IResult<&str, Expr> {
    alt((
        map(literal, |l| Expr::Literal(l)),
        map(
            separated_pair(
                identifier,
                multispace0,
                delimited(tag("("), separated_list0(
                    tag(","),
                    trim(expr),
                ), tag(")"))
            ),
            |(n, x)| Expr::Func(n, x),
        ),
        map(identifier, |i| Expr::Ident(i)),
    )).parse(i)
}

fn delim_expr(i: &str) -> IResult<&str, Expr> {
    let (i, ident) = alt((
        delimited(tag("("), trim(or_expr), tag(")")),
        ident_expr,
    )).parse(i)?;

    fold_many0(alt((
        trim(preceded(opt(trim(tag("."))), delimited(
            tag("["),
            trim(expr),
            tag("]"),
        ))),
        map(preceded(trim(tag(".")), identifier), |i| Expr::Ident(i)),
    )), move || ident.clone(), |f, i| Expr::Index(Box::new(f), Box::new(i))).parse(i)
}

fn muldiv_expr(i: &str) -> IResult<&str, Expr> {
    let (i, initial) = delim_expr(i)?;
    fold_many0(pair(trim(one_of("*/%")), delim_expr), move || initial.clone(), |acc, (o, g)| match o {
        '*' => Expr::Multiply(Box::new(acc), Box::new(g)),
        '/' => Expr::Divide(Box::new(acc), Box::new(g)),
        '%' => Expr::Modulo(Box::new(acc), Box::new(g)),
        _ => unreachable!()
    }).parse(i)
}

fn addsub_expr(i: &str) -> IResult<&str, Expr> {
    let (i, initial) = muldiv_expr(i)?;
    fold_many0(pair(trim(one_of("+-")), muldiv_expr), move || initial.clone(), |acc, (o, g)| match o {
        '+' => Expr::Add(Box::new(acc), Box::new(g)),
        '-' => Expr::Substract(Box::new(acc), Box::new(g)),
        _ => unreachable!()
    }).parse(i)
}

//
// KQL infix string operators. Parsed as keyword operators and converted to
// Func(op, [left, right]) in the AST. Word boundaries prevent matching
// inside identifiers (e.g. "has" must not match "hasPrefix").
//

fn string_op_keyword(i: &str) -> IResult<&str, &str> {
    let (rest, matched) = alt((
        tag("!contains"),
        tag("!endswith"),
        tag("!has"),
        tag("!startswith"),
        tag("contains"),
        tag("endswith"),
        tag("has"),
        tag("startswith"),
    )).parse(i)?;
    if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return Err(nom::Err::Error(nom::error::Error::new(i, nom::error::ErrorKind::Tag)));
    }
    Ok((rest, matched))
}

fn predicate(i: &str) -> IResult<&str, Expr> {
    let (i, initial) = addsub_expr(i)?;
    let (i, e) = fold_many0(pair(trim(alt((string_op_keyword, is_a("!=<>")))), addsub_expr), move || Ok(initial.clone()), |acc, (o, g)| acc.and_then(|acc| Ok(match o {
        "==" => Expr::Equals(Box::new(acc), Box::new(g)),
        "!=" => Expr::NotEquals(Box::new(acc), Box::new(g)),
        "<" => Expr::Less(Box::new(acc), Box::new(g)),
        ">" => Expr::Greater(Box::new(acc), Box::new(g)),
        "<=" => Expr::LessOrEqual(Box::new(acc), Box::new(g)),
        ">=" => Expr::GreaterOrEqual(Box::new(acc), Box::new(g)),
        _ => Expr::Func(o.to_string(), vec![acc, g]),
    }))).parse(i)?;
    Ok((i, e?))
}

//
// Word-boundary keyword matchers. Prevent `and`/`or` from matching inside
// identifiers like `android` or `oracle`.
//

fn and_keyword(i: &str) -> IResult<&str, &str> {
    let (rest, matched) = tag("and").parse(i)?;
    if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return Err(nom::Err::Error(nom::error::Error::new(i, nom::error::ErrorKind::Tag)));
    }
    Ok((rest, matched))
}

fn or_keyword(i: &str) -> IResult<&str, &str> {
    let (rest, matched) = tag("or").parse(i)?;
    if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return Err(nom::Err::Error(nom::error::Error::new(i, nom::error::ErrorKind::Tag)));
    }
    Ok((rest, matched))
}

fn and_expr(i: &str) -> IResult<&str, Expr> {
    let (i, initial) = predicate(i)?;
    fold_many0(
        preceded(trim(and_keyword), predicate),
        move || initial.clone(),
        |acc, rhs| Expr::And(Box::new(acc), Box::new(rhs)),
    ).parse(i)
}

fn or_expr(i: &str) -> IResult<&str, Expr> {
    let (i, initial) = and_expr(i)?;
    fold_many0(
        preceded(trim(or_keyword), and_expr),
        move || initial.clone(),
        |acc, rhs| Expr::Or(Box::new(acc), Box::new(rhs)),
    ).parse(i)
}

pub fn expr(i: &str) -> IResult<&str, Expr> {
    or_expr(i)
}

fn as_operator(i: &str) -> IResult<&str, (Options, String)> {
    preceded(terminated(tag("as"), multispace1), map(
        pair(opt(terminated(options, multispace1)), identifier),
        |(o, a)| (o.unwrap_or_default(), a)
    )).parse(i)
}

fn consume_operator(i: &str) -> IResult<&str, Options> {
    preceded(tag("consume"), options).parse(i)
}

fn count_operator(i: &str) -> IResult<&str, ()> {
    map(tag("count"), |_| ()).parse(i)
}

fn datatable_operator(i: &str) -> IResult<&str, (Vec<(String, Type)>, Vec<Expr>)> {
    preceded(terminated(tag("datatable"), multispace1), separated_pair(
        delimited(tag("("), type_mapping, tag(")")),
        multispace0,
        delimited(tag("["), separated_list1(tag(","), trim(expr)), tag("]"))
    )).parse(i)
}

fn distinct_operator(i: &str) -> IResult<&str, Vec<String>> {
    preceded(terminated(tag("distinct"), multispace1), separated_list1(
        tag(","),
        trim(identifier)
    )).parse(i)
}

fn evaluate_operator(i: &str) -> IResult<&str, (Options, String, Vec<Expr>)> {
    preceded(terminated(tag("evaluate"), multispace1), tuple((
        terminated(options, multispace1),
        terminated(identifier, multispace0),
        delimited(tag("("), separated_list0(tag(","), trim(expr)), tag(")"))
    ))).parse(i)
}

fn extend_operator(i: &str) -> IResult<&str, Vec<(Option<String>, Expr)>> {
    preceded(terminated(tag("extend"), multispace1), separated_list0(
        tuple((multispace0, tag(","), multispace0)),
        map(separated_pair(identifier, trim(tag("=")), expr), |(n, e)| (Some(n), e)),
    )).parse(i)
}

fn externaldata_operator(i: &str) -> IResult<&str, (Vec<(String, Type)>, Vec<String>)> {
    preceded(terminated(tag("externaldata"), multispace1), separated_pair(
        delimited(tag("("), type_mapping, tag(")")),
        multispace0,
        delimited(tag("["), separated_list1(tag(","), trim(string)), tag("]"))
    )).parse(i)
}

fn facet_operator(i: &str) -> IResult<&str, (Vec<String>, Vec<Operator>)> {
    preceded(terminated(separated_pair(tag("facet"), multispace1, tag_no_case("by")), multispace1), pair(
        separated_list0(tag(","), trim(identifier)),
        map(opt(preceded(terminated(tag("with"), multispace0), delimited(
            tag("("),
            separated_list1(tag("|"), trim(operator)),
            tag(")")
        ))), |o| o.unwrap_or_default())
    )).parse(i)
}

fn find_operator(i: &str) -> IResult<&str, (Options, (Option<Vec<Source>>, Expr), FindProjection)> {
    preceded(terminated(tag("find"), multispace1), tuple((
        terminated(options, multispace0),
        alt((
            map(separated_pair(
                preceded(
                    terminated(tag("in"), multispace1),
                    delimited(tag("("), separated_list1(tag(","), trim(source)), tag(")"))
                ),
                multispace1,
                preceded(terminated(tag("where"), multispace1), expr)
            ), |(s, e)| (Some(s), e)),
            map(expr, |e| (None, e))
        )),
        map(opt(preceded(multispace1, alt((
            map(tag("project-smart"), |_| FindProjection::ProjectSmart),
            map(preceded(terminated(tag("project"), multispace1), separated_list1(trim(tag(",")), identifier)), |c| FindProjection::Project(c))
        )))), |x| x.unwrap_or(FindProjection::ProjectSmart))
    ))).parse(i)
}

fn fork_operator(i: &str) -> IResult<&str, Vec<(Option<String>, Vec<Operator>)>> {
    preceded(terminated(tag("fork"), multispace1), separated_list1(
        tag(","),
        trim(alt((
            map(separated_pair(
                identifier,
                trim(tag("=")),
                delimited(tag("("), separated_list1(tag("|"), trim(operator)), tag(")"))
            ), |(n, e)| (Some(n), e)),
            map(delimited(tag("("), separated_list1(tag("|"), trim(operator)), tag(")")), |e| (None, e))
        )))
    )).parse(i)
}

fn getschema_operator(i: &str) -> IResult<&str, ()> {
    map(terminated(tag("getschema"), multispace1), |_| ()).parse(i)
}

//
// Parse a single join key. Supports both:
//   - `$left.X == $right.Y` (qualified form)
//   - `col` (same-name form, equivalent to $left.col == $right.col)
//

fn join_key(i: &str) -> IResult<&str, JoinKey> {
    alt((
        map(
            tuple((
                preceded(tag("$left."), identifier),
                trim(tag("==")),
                preceded(tag("$right."), identifier),
            )),
            |(left, _, right)| JoinKey { left, right },
        ),
        map(identifier, |name| JoinKey { left: name.clone(), right: name }),
    )).parse(i)
}

fn join_operator(i: &str) -> IResult<&str, (Options, TabularExpression, Vec<JoinKey>)> {
    preceded(terminated(tag("join"), multispace1), tuple((
        terminated(options, multispace0),
        terminated(delimited(trim(tag("(")), parse_query, trim(tag(")"))), multispace0),
        preceded(
            terminated(tag("on"), multispace1),
            separated_list0(tag(","), trim(join_key))
        )
    ))).parse(i)
}

fn lookup_operator(i: &str) -> IResult<&str, (Options, TabularExpression, Vec<String>)> {
    preceded(terminated(tag("lookup"), multispace1), tuple((
        terminated(options, multispace0),
        terminated(delimited(tag("("), parse_query, tag(")")), multispace0),
        preceded(
            terminated(tag("on"), multispace1),
            separated_list0(tag(","), trim(identifier))
        )
    ))).parse(i)
}

fn mv_apply_operator(i: &str) -> IResult<&str, (Vec<((String, String), Option<Type>)>, Vec<Operator>)> {
    preceded(terminated(tag("mv-apply"), multispace1), tuple((
        separated_list1(tag(","), trim(pair(
            separated_pair(trim(identifier), tag("="), trim(identifier)),
            opt(preceded(
                tuple((multispace1, tag("to"), multispace1, tag("typeof"), multispace0)),
                delimited(tag("("), trim(type_tag), tag(")"))
            ))
        ))),
        preceded(
            terminated(tag("on"), multispace1),
            delimited(tag("("), separated_list1(tag("|"), trim(operator)), tag(")"))
        )
    ))).parse(i)
}

fn mv_expand_operator(i: &str) -> IResult<&str, String> {
    preceded(terminated(tag("mv-expand"), multispace1), identifier).parse(i)
}

fn parse_operator(i: &str) -> IResult<&str, (Options, Expr, Vec<PatternToken>)> {
    preceded(terminated(tag("parse"), multispace1), tuple((
        terminated(options, multispace0),
        terminated(expr, multispace0),
        preceded(terminated(tag("with"), multispace1), pattern)
    ))).parse(i)
}

fn parse_where_operator(i: &str) -> IResult<&str, (Options, Expr, Vec<PatternToken>)> {
    preceded(terminated(tag("parse-where"), multispace1), tuple((
        terminated(options, multispace0),
        terminated(expr, multispace0),
        preceded(terminated(tag("with"), multispace1), pattern)
    ))).parse(i)
}

fn parse_kv_operator(i: &str) -> IResult<&str, (Expr, Vec<(String, Type)>, Options)> {
    preceded(terminated(tag("parse-kv"), multispace1), tuple((
        terminated(expr, multispace0),
        terminated(preceded(terminated(tag("as"), multispace0), delimited(tag("("), type_mapping, tag(")"))), multispace0),
        preceded(terminated(tag("with"), multispace1), delimited(tag("("), options_with_comma_and_quoted, tag(")"))))
    )).parse(i)
}

fn partition_operator(i: &str) -> IResult<&str, (Options, String, (Option<Source>, Vec<Operator>))> {
    preceded(terminated(tag("partition"), multispace1), tuple((
        terminated(options, multispace0),
        preceded(terminated(tag("by"), multispace1), identifier),
        alt((
            map(preceded(multispace0, delimited(tag("("), separated_list0(tag("|"), trim(operator)), tag(")"))), |o| (None, o)),
            map(separated_pair(source, multispace0, many0(preceded(tag("|"), trim(operator)))), |(s, o)| (Some(s), o))
        ))
    ))).parse(i)
}

fn print_operator(i: &str) -> IResult<&str, Vec<(Option<String>, Expr)>> {
    preceded(terminated(tag("print"), multispace0), separated_list0(
        tag(","),
        trim(alt((
            map(separated_pair(identifier, trim(tag("=")), expr), |(n, e)| (Some(n), e)),
            map(expr, |e| (None, e))
        )))
    )).parse(i)
}

fn project_operator(i: &str) -> IResult<&str, Vec<(Option<String>, Expr)>> {
    preceded(terminated(tag("project"), multispace1), separated_list0(
        tag(","),
        trim(alt((
            map(separated_pair(identifier, trim(tag("=")), expr), |(n, e)| (Some(n), e)),
            map(expr, |e| (None, e))
        ))),
    )).parse(i)
}

fn project_away_operator(i: &str) -> IResult<&str, Vec<String>> {
    preceded(terminated(tag("project-away"), multispace1), separated_list1(
        tag(","),
        trim(identifier)
    )).parse(i)
}

fn project_keep_operator(i: &str) -> IResult<&str, Vec<String>> {
    preceded(terminated(tag("project-keep"), multispace1), separated_list1(
        tag(","),
        trim(identifier)
    )).parse(i)
}

fn project_rename_operator(i: &str) -> IResult<&str, Vec<(String, String)>> {
    preceded(terminated(tag("project-rename"), multispace1), separated_list1(
        tag(","),
        separated_pair(trim(identifier), tag("="), trim(identifier))
    )).parse(i)
}

fn project_reorder_operator(i: &str) -> IResult<&str, Vec<(String, Option<(bool, bool)>)>> {
    preceded(terminated(tag("project-reorder"), multispace1), separated_list1(
        tag(","),
        trim(pair(wildcard_identifier, opt(preceded(multispace1, alt((
            value((true, false), tag("asc")),
            value((false, false), tag("desc")),
            value((true, true), tag("granny-asc")),
            value((false, true), tag("granny-desc"))
        ))))))
    )).parse(i)
}

fn where_operator(i: &str) -> IResult<&str, Expr> {
    preceded(terminated(tag("where"), multispace1), expr).parse(i)
}

fn range_operator(i: &str) -> IResult<&str, (String, Expr, Expr, Expr)> {
    preceded(terminated(tag("range"), multispace1), tuple((
        terminated(identifier, multispace1),
        terminated(preceded(terminated(tag("from"), multispace1), expr), multispace1),
        terminated(preceded(terminated(tag("to"), multispace1), expr), multispace1),
        preceded(terminated(tag("step"), multispace1), expr)
    ))).parse(i)
}

fn reduce_operator(i: &str) -> IResult<&str, (Options, Expr, Option<Options>)> {
    preceded(terminated(tag("reduce"), multispace1), tuple((
        terminated(options, multispace0),
        terminated(preceded(terminated(tag("by"), multispace1), expr), multispace0),
        opt(preceded(terminated(tag("with"), multispace1), options_with_comma_and_quoted))
    ))).parse(i)
}

fn render_operator(i: &str) -> IResult<&str, (String, Option<Options>)> {
    preceded(terminated(tag("render"), multispace1), tuple((
        terminated(identifier, multispace0),
        opt(preceded(terminated(tag("with"), multispace1), delimited(tag("("), options_with_comma_and_quoted, tag(")")))))
    )).parse(i)
}

fn sample_operator(i: &str) -> IResult<&str, u32> {
    preceded(
        terminated(tag("sample"), multispace1),
        u32
    ).parse(i)
}

fn sample_distinct_operator(i: &str) -> IResult<&str, (u32, String)> {
    preceded(
        terminated(tag("sample-distinct"), multispace1),
        separated_pair(
            u32,
            delimited(multispace1, tag("by"), multispace1),
            identifier
        )
    ).parse(i)
}

fn serialize_operator(i: &str) -> IResult<&str, Vec<(Option<String>, Expr)>> {
    preceded(terminated(tag("serialize"), multispace1), separated_list0(
        tag(","),
        trim(map(
            separated_pair(identifier, trim(tag("=")), expr),
            |(n, e)| (Some(n), e)
        )),
    )).parse(i)
}

fn summarize_operator(i: &str) -> IResult<&str, (Vec<(Option<String>, Expr)>, Vec<Expr>)> {
    preceded(terminated(tag("summarize"), multispace1), pair(
        separated_list0(tag(","), trim(alt((
            map(separated_pair(identifier, trim(tag("=")), expr), |(n, e)| (Some(n), e)),
            map(expr, |e| (None, e))
        )))),
        map(opt(preceded(
            terminated(tag("by"), multispace1),
            separated_list1(tag(","), trim(expr))
        )), |b| b.unwrap_or_default())
    )).parse(i)
}

fn sort_operator(i: &str) -> IResult<&str, Vec<String>> {
    preceded(tuple((tag("sort"), multispace1, tag("by"))), separated_list1(
        tag(","),
        trim(identifier)
    )).parse(i)
}

fn take_operator(i: &str) -> IResult<&str, u32> {
    preceded(
        terminated(alt((tag("take"), tag("limit"))), multispace1),
        u32
    ).parse(i)
}

fn top_operator(i: &str) -> IResult<&str, (u32, Expr, bool, bool)> {
    map(preceded(
        terminated(tag("top"), multispace1),
        tuple((
            terminated(u32, multispace1),
            preceded(terminated(tag("by"), multispace1), trim(expr)),
            opt(terminated(alt((
                value(true, tag("asc")),
                value(false, tag("desc"))
            )), multispace0)),
            opt(preceded(terminated(tag("nulls"), multispace1), alt((
                value(true, tag("first")),
                value(false, tag("last"))
            )))),
        ))
    ), |(n, e, s, o)| (n, e, s.unwrap_or(false), o.unwrap_or(s.unwrap_or(false)))).parse(i)
}

fn union_operator(i: &str) -> IResult<&str, (Options, Vec<Source>)> {
    preceded(terminated(tag("union"), multispace1), tuple((
        terminated(options, multispace0),
        separated_list1(trim(tag(",")), alt((
            delimited(tag("("), trim(source), tag(")")),
            map(identifier, |e| Source::Reference(e))
        )))
    ))).parse(i)
}

fn operator(i: &str) -> IResult<&str, Operator> {
    alt((
        alt((
            map(as_operator, |(o, a)| Operator::As(o, a)),
            map(consume_operator, |o| Operator::Consume(o)),
            map(count_operator, |_| Operator::Count),
            map(distinct_operator, |c| Operator::Distinct(c)),
            map(evaluate_operator, |(o, n, x)| Operator::Evaluate(o, n, x)),
            map(extend_operator, |e| Operator::Extend(e)),
            map(facet_operator, |(a, g)| Operator::Facet(a, g)),
            map(fork_operator, |f| Operator::Fork(f)),
            map(getschema_operator, |_| Operator::Getschema),
            map(join_operator, |(o, a, g)| Operator::Join(o, a, g)),
            map(lookup_operator, |(o, a, g)| Operator::Lookup(o, a, g)),
        )),
        alt((
            map(mv_apply_operator, |(a, g)| Operator::MvApply(a, g)),
            map(mv_expand_operator, |e| Operator::MvExpand(e)),
        )),
        alt((
            map(project_operator, |p| Operator::Project(p)),
            map(project_away_operator, |p| Operator::ProjectAway(p)),
            map(project_keep_operator, |p| Operator::ProjectKeep(p)),
            map(project_rename_operator, |p| Operator::ProjectRename(p)),
            map(project_reorder_operator, |p| Operator::ProjectReorder(p))
        )),
        alt((
            map(parse_operator, |(o, e, p)| Operator::Parse(o, e, p)),
            map(parse_where_operator, |(o, e, p)| Operator::ParseWhere(o, e, p)),
            map(parse_kv_operator, |(e, t, o)| Operator::ParseKV(e, t, o)),
        )),
        map(partition_operator, |(o, a, (s, g))| Operator::Partition(o, a, s, g)),
        map(reduce_operator, |(o, e, p)| Operator::Reduce(o, e, p)),
        map(render_operator, |(v, p)| Operator::Render(v, p)),
        alt((
            map(sample_operator, |s| Operator::Sample(s)),
            map(sample_distinct_operator, |(s, c)| Operator::SampleDistinct(s, c))
        )),
        alt((
            map(serialize_operator, |e| Operator::Serialize(e)),
            map(summarize_operator, |(a, g)| Operator::Summarize(a, g)),
            map(sort_operator, |o| Operator::Sort(o)),
        )),
        alt((
            map(take_operator, |t| Operator::Take(t)),
            map(top_operator, |(n, e, s, o)| Operator::Top(n, e, s, o))
        )),
        map(union_operator, |(o, s)| Operator::Union(o, s)),
        map(where_operator, |e| Operator::Where(e))
    )).parse(i)
}

fn source(i: &str) -> IResult<&str, Source> {
    alt((
        map(datatable_operator, |(a, g)| Source::Datatable(a, g)),
        map(externaldata_operator, |(t, c)| Source::Externaldata(t, c)),
        map(find_operator, |(o, (s, e), p)| Source::Find(o, s, e, p)),
        map(print_operator, |e| Source::Print(e)),
        map(range_operator, |(c, f, t, s)| Source::Range(c, f, t, s)),
        map(union_operator, |(o, s)| Source::Union(o, s)),
        map(identifier, |e| Source::Reference(e))
    )).parse(i)
}

pub fn parse_query(i: &str) -> IResult<&str, TabularExpression> {
    map(separated_pair(source, multispace0, many0(preceded(tag("|"), trim(operator)))),
    |(source, operators)| TabularExpression {
        source,
        operators
    }).parse(i)
}

fn parse_let(i: &str) -> IResult<&str, (String, LetExpression)> {
    preceded(
        terminated(tag("let"), multispace1),
        separated_pair(
            trim(identifier),
            tag("="),
            trim(alt((
                map(expr, |e| LetExpression::Scalar(e)),
                map(parse_query, |e| LetExpression::Tabular(e)),
            )))
        )
    ).parse(i)
}

pub fn parse(i: &str) -> IResult<&str, Vec<Statement>> {
    separated_list1(
        tag(";"),
        trim(alt((
            map(parse_let, |(n, e)| Statement::Let(n, e)),
            map(parse_query, |e| Statement::TabularExpression(e)),
        ))),
    ).parse(i)
}
