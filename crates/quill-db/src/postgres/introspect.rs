//! What is in a PostgreSQL server, asked one level at a time.
//!
//! The catalogue tables rather than `information_schema`: the views are portable and slow — every one
//! of them joins half a dozen catalogue relations and applies a permission filter — and this client is
//! not portable anyway, because SQLite has its own module beside this one. `pg_class`, `pg_namespace`,
//! `pg_attribute` and `pg_index` answer all four questions the tree asks.
//!
//! Every value is bound as a parameter, so a schema or a table whose name contains a quote is a
//! non-event here as it is everywhere else in this crate.

use crate::catalog::{Item, Kind, Table};
use crate::postgres::session::Session;
use crate::rows::{Answer, Failure};
use crate::value::{Column, Value};

/// The databases on this server that can be connected to.
///
/// Templates and databases that refuse connections are left out, because a row in the tree that
/// cannot be opened is a row that only ever disappoints.
pub fn databases(session: &mut Session) -> Answer<Vec<String>> {
    let rows = session.simple(
        "select datname from pg_database where datallowconn and not datistemplate order by datname",
        usize::MAX,
    )?;
    Ok(rows.rows.iter().filter_map(|row| row.first()?.text().map(str::to_owned)).collect())
}

/// The schemas of the database this connection is on.
///
/// `pg_catalog` and `information_schema` are listed last rather than hidden: The reference editor shows them, they
/// are where somebody goes to answer a question about the server itself, and a tree that quietly left
/// out two schemas would be a tree that disagrees with `\dn`.
pub fn schemas(session: &mut Session) -> Answer<Vec<String>> {
    let rows = session.simple(
        "select nspname, \
           case when nspname in ('pg_catalog','information_schema') or nspname like 'pg\\_%' \
                then 1 else 0 end as system \
         from pg_namespace \
         where nspname not like 'pg\\_toast%' and nspname not like 'pg\\_temp%' \
         order by system, nspname",
        usize::MAX,
    )?;
    Ok(rows.rows.iter().filter_map(|row| row.first()?.text().map(str::to_owned)).collect())
}

/// Everything in one schema that the tree draws a row for.
pub fn items(session: &mut Session, schema: &str) -> Answer<Vec<Item>> {
    let rows = session.extended(
        "select c.relname, c.relkind \
         from pg_class c join pg_namespace n on n.oid = c.relnamespace \
         where n.nspname = $1 and c.relkind in ('r','p','v','m','f','S') \
         order by c.relkind, c.relname",
        &[Value::typed(schema)],
        usize::MAX,
    )?;
    Ok(rows
        .rows
        .iter()
        .filter_map(|row| {
            let name = row.first()?.text()?.to_owned();
            let kind = match row.get(1)?.text()? {
                // `p` is a partitioned table, which is a table as far as anybody reading it is
                // concerned.
                "r" | "p" => Kind::Table,
                "v" => Kind::View,
                "m" => Kind::MaterialisedView,
                "f" => Kind::Foreign,
                "S" => Kind::Sequence,
                _ => return None,
            };
            Some(Item { name, kind })
        })
        .collect())
}

/// The routines in a schema, which hang under their own folder.
///
/// Asked separately from [`items`] because `pg_proc` is a different relation and because a schema with
/// three hundred functions in it should not make listing its tables slower.
pub fn routines(session: &mut Session, schema: &str) -> Answer<Vec<Item>> {
    let rows = session.extended(
        "select p.proname from pg_proc p join pg_namespace n on n.oid = p.pronamespace \
         where n.nspname = $1 order by p.proname",
        &[Value::typed(schema)],
        usize::MAX,
    )?;
    Ok(rows
        .rows
        .iter()
        .filter_map(|row| Some(Item { name: row.first()?.text()?.to_owned(), kind: Kind::Routine }))
        .collect())
}

/// One table's columns, with its primary key marked.
///
/// The key is the whole point of this query. Whether a row can be changed is decided by whether it can
/// be addressed, and the columns of the primary key are what addresses it — see
/// `catalog::Table::can_be_changed`.
pub fn table(session: &mut Session, schema: &str, name: &str) -> Answer<Table> {
    let rows = session.extended(
        "select a.attname, \
                format_type(a.atttypid, a.atttypmod) as kind, \
                a.attnotnull, \
                coalesce(k.is_key, false) as in_key, \
                coalesce(k.at, 0) as key_at \
         from pg_attribute a \
         join pg_class c on c.oid = a.attrelid \
         join pg_namespace n on n.oid = c.relnamespace \
         left join ( \
           select i.indrelid, u.attnum, u.at, true as is_key \
           from pg_index i, unnest(i.indkey) with ordinality as u(attnum, at) \
           where i.indisprimary \
         ) k on k.indrelid = c.oid and k.attnum = a.attnum \
         where n.nspname = $1 and c.relname = $2 and a.attnum > 0 and not a.attisdropped \
         order by a.attnum",
        &[Value::typed(schema), Value::typed(name)],
        usize::MAX,
    )?;
    if rows.rows.is_empty() {
        return Err(Failure::said(format!("{schema}.{name} has no columns, or is not there any more.")));
    }
    let mut table = Table { schema: schema.to_owned(), name: name.to_owned(), ..Table::default() };
    // The key is gathered in the index's own order, which is the order it has to be written in.
    let mut key: Vec<(i64, String)> = Vec::new();
    for row in &rows.rows {
        let name = row.first().and_then(crate::value::Value::text).unwrap_or_default().to_owned();
        let type_name = row.get(1).and_then(crate::value::Value::text).unwrap_or_default().to_owned();
        let not_null = row.get(2).and_then(crate::value::Value::text) == Some("t");
        let in_key = row.get(3).and_then(crate::value::Value::text) == Some("t");
        let at: i64 = row.get(4).and_then(crate::value::Value::text).and_then(|at| at.parse().ok()).unwrap_or(0);
        let mut column = Column::new(&name, type_name);
        column.not_null = not_null;
        column.in_key = in_key;
        if in_key {
            key.push((at, name));
        }
        table.columns.push(column);
    }
    key.sort_by_key(|(at, _)| *at);
    table.key = key.into_iter().map(|(_, name)| name).collect();
    Ok(table)
}

/// The `CREATE` statement for something in the database.
///
/// **A view's definition is the server's own** — `pg_get_viewdef` is what the server would print — and
/// a table's is composed here from the catalogue, with a line saying so. PostgreSQL keeps no original
/// text for a table, so anything claiming to be one would be a reconstruction pretending not to be;
/// `pg_dump` is the program that does this properly and it is a program that exists.
pub fn ddl(session: &mut Session, schema: &str, name: &str, kind: Kind) -> Answer<String> {
    if matches!(kind, Kind::View | Kind::MaterialisedView) {
        let rows = session.extended(
            "select pg_get_viewdef(c.oid, true) \
             from pg_class c join pg_namespace n on n.oid = c.relnamespace \
             where n.nspname = $1 and c.relname = $2",
            &[Value::typed(schema), Value::typed(name)],
            usize::MAX,
        )?;
        let body = rows
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(crate::value::Value::text)
            .unwrap_or_default();
        return Ok(format!("CREATE VIEW {schema}.{name} AS\n{body}"));
    }
    let table = table(session, schema, name)?;
    let mut out = String::new();
    out.push_str("-- Composed by Quill from the catalogue. PostgreSQL keeps no original text for a\n");
    out.push_str("-- table, so this is what the columns say rather than what was typed. `pg_dump` is\n");
    out.push_str("-- the program that reproduces one exactly.\n");
    out.push_str(&format!("CREATE TABLE {}.{} (\n", crate::catalog::quoted(schema, '"'), crate::catalog::quoted(name, '"')));
    let mut lines: Vec<String> = table
        .columns
        .iter()
        .map(|column| {
            format!(
                "    {} {}{}",
                crate::catalog::quoted(&column.name, '"'),
                column.type_name,
                match column.not_null {
                    true => " NOT NULL",
                    false => "",
                }
            )
        })
        .collect();
    if !table.key.is_empty() {
        lines.push(format!(
            "    PRIMARY KEY ({})",
            table.key.iter().map(|name| crate::catalog::quoted(name, '"')).collect::<Vec<String>>().join(", ")
        ));
    }
    out.push_str(&lines.join(",\n"));
    out.push_str("\n);\n");
    Ok(out)
}
