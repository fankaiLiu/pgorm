#[derive(Debug)]
pub(super) enum SqlPart {
    Raw(String),
    Param,
}
