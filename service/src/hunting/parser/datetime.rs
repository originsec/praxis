#![allow(deprecated)]

use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::{u32, alpha1, digit1, multispace0, multispace1, one_of};
use nom::combinator::{map, map_res, opt, value};
use nom::sequence::{pair, preceded, terminated, tuple};
use nom::{IResult, Parser};

use super::ast::DateTime;

struct ParsedDate {
    year: u32,
    month: u32,
    day: u32
}

struct ParsedTime {
    hour: u32,
    minute: u32,
    second: u32
}

pub fn iso8601_datetime(input: &str) -> IResult<&str, DateTime> {
    let (input, (date, _, time, timezone)) = tuple((
        iso8601_date,
        alt((multispace1, tag("T"))),
        iso8601_time,
        opt(preceded(multispace0, iso8601_timezone)),
    )).parse(input)?;

    Ok((
        input,
        DateTime {
            year: date.year,
            month: date.month,
            day: date.day,
            hour: time.hour,
            minute: time.minute,
            second: time.second,
            timezone,
        },
    ))
}

fn iso8601_date(input: &str) -> IResult<&str, ParsedDate> {
    let (input, (year, _, month, _, day)) = tuple((
        u32,
        tag("-"),
        u32,
        tag("-"),
        u32,
    )).parse(input)?;

    Ok((
        input,
        ParsedDate {
            year,
            month,
            day,
        },
    ))
}

fn iso8601_time(input: &str) -> IResult<&str, ParsedTime> {
    map(tuple((
        u32,
        preceded(tag(":"), u32),
        opt(preceded(tag(":"), u32)),
    )), |(hour, minute, second)| ParsedTime {
        hour,
        minute,
        second: second.unwrap_or(0),
    }).parse(input)
}

fn iso8601_timezone(input: &str) -> IResult<&str, String> {
    alt((
        map(pair(one_of("+-"), digit1), |(sign, value)| -> String {
            format!("{}{}", sign, value)
        }),
        map_res(pair(opt(one_of("+-")), digit1), |(sign, value): (Option<char>, &str)| -> Result<String, nom::error::Error<&str>> {
            Ok(format!("{}{}", sign.unwrap_or('+'), value))
        }),
    )).parse(input)
}

fn rfc822_date(input: &str) -> IResult<&str, ParsedDate> {
    map(tuple((
        u32,
        multispace1,
        month,
        multispace1,
        u32
    )), |(day, _, month, _, year)| ParsedDate {
        year,
        month,
        day,
    }).parse(input)
}

pub fn rfc822_datetime(input: &str) -> IResult<&str, DateTime> {
    map(tuple((
        opt(terminated(alpha1, tag(","))), // Optional day name
        multispace0,
        rfc822_date,
        multispace0,
        time,
        multispace0,
        rfc822_timezone,
    )), |(_, _, date, _, time, _, timezone)| DateTime {
        year: date.year,
        month: date.month,
        day: date.day,
        hour: time.hour,
        minute: time.minute,
        second: time.second,
        timezone: Some(timezone),
    }).parse(input)
}

fn rfc822_timezone(input: &str) -> IResult<&str, String> {
    alt((
        map(pair(one_of("+-"), digit1), |(sign, value)| {
            format!("{}{}", sign, value)
        }),
        map(pair(opt(one_of("+-")), digit1), |(sign, value)| {
            format!("{}{}", sign.unwrap_or('+'), value)
        })
    )).parse(input)
}

pub fn rfc850_datetime(input: &str) -> IResult<&str, DateTime> {
    let (input, (_, _, date, _, time, _, timezone)) = tuple((
        opt(terminated(alpha1, tag(","))), // Optional day name
        multispace0,
        rfc850_date,
        multispace0,
        time,
        multispace0,
        rfc850_timezone,
    )).parse(input)?;

    Ok((
        input,
        DateTime {
            year: date.year,
            month: date.month,
            day: date.day,
            hour: time.hour,
            minute: time.minute,
            second: 0,
            timezone: Some(timezone),
        },
    ))
}

fn rfc850_date(input: &str) -> IResult<&str, ParsedDate> {
    map(tuple((
        u32,
        tag("-"),
        month,
        tag("-"),
        u32
    )), |(day, _, month, _, year)| ParsedDate {
        year,
        month,
        day,
    }).parse(input)
}

fn rfc850_timezone(input: &str) -> IResult<&str, String> {
    alt((
        map(pair(one_of("+-"), digit1), |(sign, value)| {
            format!("{}{}", sign, value)
        }),
        map(pair(opt(one_of("+-")), digit1), |(sign, value)| {
            format!("{}{}", sign.unwrap_or('+'), value)
        }),
    )).parse(input)
}

fn time(input: &str) -> IResult<&str, ParsedTime> {
    map(tuple((
        u32,
        preceded(tag(":"), u32),
        opt(preceded(tag(":"), u32)),
    )), |(hour, minute, second)| ParsedTime {
        hour,
        minute,
        second: second.unwrap_or(0),
    }).parse(input)
}

fn month(input: &str) -> IResult<&str, u32> {
    alt((
        alt((
            value(1, tag("Jan")),
            value(1, tag("January")),
            value(2, tag("Feb")),
            value(2, tag("February")),
            value(3, tag("Mar")),
            value(3, tag("March")),
            value(4, tag("Apr")),
            value(4, tag("April")),
            value(5, tag("May")),
            value(6, tag("Jun")),
            value(6, tag("June"))
        )),
        alt((
            value(7, tag("Jul")),
            value(7, tag("July")),
            value(8, tag("Aug")),
            value(8, tag("August")),
            value(9, tag("Sep")),
            value(9, tag("September")),
            value(10, tag("Oct")),
            value(10, tag("October")),
            value(11, tag("Nov")),
            value(11, tag("November")),
            value(12, tag("Dec")),
            value(12, tag("December"))
        ))
    )).parse(input)
}
