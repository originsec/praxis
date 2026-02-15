use std::collections::HashMap;

pub type Options = HashMap<String, OptionLiteral>;

#[derive(Debug, PartialEq)]
pub enum Statement {
    TabularExpression(TabularExpression),
    Let(String, LetExpression)
}

#[derive(Debug, PartialEq)]
pub struct TabularExpression {
    pub source: Source,
    pub operators: Vec<Operator>
}

#[derive(Debug, PartialEq)]
pub enum Source {
    Datatable(Vec<(String, Type)>, Vec<Expr>),
    Externaldata(Vec<(String, Type)>, Vec<String>),
    Find(Options, Option<Vec<Source>>, Expr, FindProjection),
    Print(Vec<(Option<String>, Expr)>),
    Range(String, Expr, Expr, Expr),
    Reference(String),
    Union(Options, Vec<Source>)
}

#[derive(Debug, PartialEq)]
pub struct JoinKey {
    pub left: String,
    pub right: String,
}

#[derive(Debug, PartialEq)]
pub enum Operator {
    As(Options, String),
    Consume(Options),
    Count,
    Distinct(Vec<String>),
    Evaluate(Options, String, Vec<Expr>),
    Extend(Vec<(Option<String>, Expr)>),
    Facet(Vec<String>, Vec<Operator>),
    Fork(Vec<(Option<String>, Vec<Operator>)>),
    Getschema,
    Join(Options, TabularExpression, Vec<JoinKey>),
    Lookup(Options, TabularExpression, Vec<String>),
    MvApply(Vec<((String, String), Option<Type>)>, Vec<Operator>),
    MvExpand(String),
    Parse(Options, Expr, Vec<PatternToken>),
    ParseWhere(Options, Expr, Vec<PatternToken>),
    ParseKV(Expr, Vec<(String, Type)>, Options),
    Partition(Options, String, Option<Source>, Vec<Operator>),
    Project(Vec<(Option<String>, Expr)>),
    ProjectAway(Vec<String>),
    ProjectKeep(Vec<String>),
    ProjectRename(Vec<(String, String)>),
    ProjectReorder(Vec<(String, Option<(bool, bool)>)>),
    Reduce(Options, Expr, Option<Options>),
    Render(String, Option<Options>),
    Sample(u32),
    SampleDistinct(u32, String),
    Serialize(Vec<(Option<String>, Expr)>),
    Summarize(Vec<(Option<String>, Expr)>, Vec<Expr>),
    Sort(Vec<String>),
    Take(u32),
    Top(u32, Expr, bool, bool),
    Union(Options, Vec<Source>),
    Where(Expr)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Ident(String),
    Index(Box<Expr>, Box<Expr>),
    Literal(Literal),
    Equals(Box<Expr>, Box<Expr>),
    NotEquals(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Substract(Box<Expr>, Box<Expr>),
    Multiply(Box<Expr>, Box<Expr>),
    Divide(Box<Expr>, Box<Expr>),
    Modulo(Box<Expr>, Box<Expr>),
    Less(Box<Expr>, Box<Expr>),
    Greater(Box<Expr>, Box<Expr>),
    LessOrEqual(Box<Expr>, Box<Expr>),
    GreaterOrEqual(Box<Expr>, Box<Expr>),
    Func(String, Vec<Expr>)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Bool,
    DateTime,
    Decimal,
    Dynamic,
    Int,
    Long,
    Real,
    String,
    Timespan
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum Literal {
    Bool(Option<bool>),
    DateTime(Option<DateTime>),
    Decimal(Option<f64>),
    Dynamic(Option<Dynamic>),
    Int(Option<i32>),
    Long(Option<i64>),
    Real(Option<f32>),
    String(String),
    Timespan(Option<i64>)
}

#[derive(Debug, Clone, PartialEq)]
pub enum OptionLiteral {
    Bool(bool),
    Long(i64),
    String(String),
    Identifier(String)
}

#[derive(Debug, Clone, PartialEq)]
pub struct DateTime {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dynamic {
    Array(Vec<Option<Dynamic>>),
    Bool(Option<bool>),
    DateTime(Option<DateTime>),
    Decimal(Option<f64>),
    Dictionary(HashMap<String, Option<Dynamic>>),
    Int(Option<i32>),
    Long(Option<i64>),
    Real(Option<f32>),
    String(String),
    Timespan(Option<i64>)
}

#[derive(Debug, PartialEq)]
pub enum FindProjection {
    ProjectSmart,
    Project(Vec<String>)
}

#[derive(Debug, PartialEq)]
pub enum PatternToken {
    Wildcard,
    String(String),
    Column(String, Option<Type>),
}

#[derive(Debug, PartialEq)]
pub enum LetExpression {
    Tabular(TabularExpression),
    Scalar(Expr)
}
