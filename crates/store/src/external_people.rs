use rusqlite::params;

use crate::error::Result;
use crate::store::Store;

impl Store {
    /// People of other organizations, with their organization's name when Teams gave one.
    pub fn external_people(&self) -> Result<Vec<(String, Option<String>)>> {
        let connection = self.read()?;
        let mut statement =
            connection.prepare_cached("SELECT user_id, tenant_name FROM external_people")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn upsert_external_people(&self, people: &[(String, Option<String>)]) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO external_people (user_id, tenant_name) VALUES (?1, ?2)
                 ON CONFLICT (user_id) DO UPDATE SET
                    tenant_name = COALESCE(excluded.tenant_name, external_people.tenant_name)",
            )?;
            for (user_id, tenant_name) in people {
                statement.execute(params![user_id, tenant_name])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::store::Store;

    #[test]
    fn an_external_person_keeps_a_known_organization() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_external_people(&[("ann".into(), Some("Example Corp".into()))])
            .unwrap();
        store
            .upsert_external_people(&[("ann".into(), None)])
            .unwrap();
        assert_eq!(
            store.external_people().unwrap(),
            [("ann".to_owned(), Some("Example Corp".to_owned()))]
        );
    }
}
