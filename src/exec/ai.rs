//! AI metadata uses a separate pool lease and never a user's transaction.
use super::sink::Captured;
use super::{Manager, ResultSink, StatementKey};
use crate::ai::schema::{Builder, Event};
use crate::catalog::RelationRef;
use crate::connector::{Session, SessionContext};
use crate::grid::Budget;
use crate::structure::{Scope, Section, structure_sql};
use std::sync::{Arc, Mutex, mpsc};
use tokio_util::sync::CancellationToken;

async fn query(
    session: &mut dyn Session,
    sql: &str,
    budget: &Arc<Budget>,
    events: &tokio::sync::mpsc::Sender<super::ExecEvent>,
) -> Result<Captured, String> {
    let capture = Arc::new(Mutex::new(Captured::default()));
    let mut sink = ResultSink::new(
        StatementKey {
            console: 0,
            execution: 0,
            index: 0,
        },
        events.clone(),
        budget.clone(),
        usize::MAX,
    );
    sink.capture_to(capture.clone());
    session.run(sql, sink).await.map_err(|_| {
        "Cannot read complete schema metadata. Check database permissions and refresh.".to_owned()
    })?;
    let result = std::mem::take(
        &mut *capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    if result.incomplete {
        return Err(
            "The schema read exceeds the memory budget. Limit the connection schemas.".into(),
        );
    }
    Ok(result)
}
impl Manager {
    pub fn load_ai_schema(
        &self,
        connection: u64,
        generation: u64,
        schemas: Vec<String>,
        cancel: CancellationToken,
        sender: mpsc::Sender<Event>,
    ) {
        let binding = self.connections.get(&connection).cloned();
        let budget = self.budget.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let operation=async {
                let binding=binding.ok_or("The connection is not available. Open it again.")?;
                let _=sender.send(Event::Waiting(connection,generation));
                let mut lease=match binding.pool.acquire().await {
                    Ok(lease)=>lease,
                    Err(error)=>{
                        if let Some(event)=binding.refusal(&error) {let _=events.send(event).await;}
                        return Err("Cannot open the metadata session. Check database access.".to_owned());
                    }
                };
                let _=sender.send(Event::Reading(connection,generation));
                let dialect=binding.pool.dialect();
                let read=async {
                    lease.session().apply_context(&SessionContext {read_only:true,..Default::default()}).await.map_err(|_| "Cannot prepare the metadata session.")?;
                    let catalog=lease.session().read_catalog_of(&schemas).await.map_err(|_| "Cannot read the catalog. Check access and refresh.")?;
                    let mut builder=Builder::new(&catalog,&budget)?;
                    for schema in &catalog.schemas {
                        lease.session().apply_context(&SessionContext {read_only:true,schema:Some(schema.name.clone()),only_schema:true}).await.map_err(|_| "Cannot select the metadata schema.")?;
                        let relation=RelationRef {schema:schema.name.clone(),relation:String::new()};
                        let mut queries=vec![("columns",structure_sql(dialect,Scope::Schema,&relation,Section::Columns).ok_or("This source has no SQL metadata.")?,true)];
                        for (index,sql) in crate::compare::sql::compare_sql(dialect,&schema.name).unwrap_or_default().into_iter().enumerate() {
                            queries.push((match index {0=>"columns",1=>"constraints",2=>"indexes",_=>"definition"},sql,false));
                        }
                        if let Some(sql)=crate::ai::schema::comments_sql(dialect,&schema.name) { queries.push(("comments",sql,false)); }
                        for (section,sql,all_columns) in queries {
                            let captured=query(lease.session(),&sql,&budget,&events).await?;
                            for row in &captured.rows { builder.add(&schema.name,section,&captured.columns,&row.row,all_columns)?; }
                        }
                    }
                    Ok::<_,String>(Arc::new(builder.finish()))
                };
                let limit = binding.settings().sql_timeout;
                let result=match limit {
                    Some(limit)=>tokio::time::timeout(limit,read).await.map_err(|_| "The schema time limit ended. Limit schemas or increase the database timeout.".to_owned())?,
                    None=>read.await,
                };
                if result.is_ok() { lease.release().await; }
                result
            };
            let result=tokio::select! { result=operation=>result, ()=cancel.cancelled()=>Err("Schema read cancelled.".into()) };
            let _=sender.send(Event::Loaded(connection,generation,result));
        });
    }

    pub fn spawn_ai(&self, operation: impl std::future::Future<Output = ()> + Send + 'static) {
        self.runtime.spawn(operation);
    }
}
