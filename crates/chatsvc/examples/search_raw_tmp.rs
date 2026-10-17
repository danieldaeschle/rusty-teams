use serde_json::{Value, json};
use session::{DEFAULT_ENDPOINT, Method, Request, Scope, Session};

fn query(text: &str) -> Value {
    json!({"QueryString": text, "DisplayQueryString": text, "NormalizedQueryString": text})
}

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let out = std::env::args().nth(1).expect("out dir");
    let text = std::env::args().nth(2).unwrap_or_else(|| "christian".into());
    let anchor_mode = std::env::args().nth(3).unwrap_or_else(|| "oid".into());
    let session = Session::connect(&endpoint).await.expect("browser");
    let me = session.request(Method::Get, "https://graph.microsoft.com/v1.0/me?$select=id", &Scope::graph("User.Read"), None).await.expect("me");
    let org = session.request(Method::Get, "https://graph.microsoft.com/v1.0/organization?$select=id", &Scope::graph("User.Read"), None).await.expect("org");
    let oid = me.body["id"].as_str().unwrap().to_owned();
    let tid = org.body["value"][0]["id"].as_str().unwrap().to_owned();
    let scope = Scope::new("https://substrate.office.com", "SubstrateSearch-Internal.ReadWrite");
    let mut headers = vec![
        ("x-client-version".to_owned(), "T2.1".to_owned()),
        ("X-ClientType".to_owned(), "MicrosoftTeams".to_owned()),
        ("X-ClientScenario".to_owned(), "TeamsSearch".to_owned()),
        ("client-request-id".to_owned(), uuid::Uuid::new_v4().to_string()),
    ];
    if anchor_mode == "oid" {
        headers.push(("x-anchormailbox".to_owned(), format!("Oid:{oid}@{tid}")));
        headers.push(("x-routingparameter-sessionkey".to_owned(), format!("OID:{oid}@{tid}")));
    }
    let cvid = uuid::Uuid::new_v4().to_string();
    let base = "https://substrate.office.com";
    let mut requests = Vec::new();
    let mut init = Request::get(format!("{base}/search/api/v2/init?cvid={cvid}&scenario=powerbar"));
    init.headers = headers.clone();
    requests.push(("init", init));
    let mut zero = Request::get(format!("{base}/search/api/v1/suggestions?scenario=powerbar&query=&cvid={cvid}&logicalId={}", uuid::Uuid::new_v4()));
    zero.headers = headers.clone();
    requests.push(("zero", zero));
    let q = query(&text);
    let body = json!({"EntityRequests":[
        {"EntityType":"People","Size":5,"Query":q,"Fields":["Id","MRI","DisplayName","EmailAddresses","PeopleType","PeopleSubtype","UserPrincipalName","GivenName","Surname","CompanyName","JobTitle","ExternalDirectoryObjectId","Department"],"Provenances":["Mailbox","Directory"],"From":0},
        {"EntityType":"File","Size":3,"Query":q},
        {"EntityType":"Chat","Size":5,"Query":q},
        {"EntityType":"Team","Size":3,"Query":q},
        {"EntityType":"Channel","Size":4,"Query":q},
        {"EntityType":"Text","Size":2,"Query":q},
        {"EntityType":"Message","Size":11,"Query":q},
        {"EntityType":"Event","Size":3,"Query":q}],
      "Scenario":{"Name":"powerbar","Dimensions":[{"DimensionName":"QueryType","DimensionValue":"PeopleCentricSearch"}]},
      "Cvid":cvid,"LogicalId":uuid::Uuid::new_v4().to_string(),"AppName":"Microsoft Teams","dataSource":"personScoped"});
    let mut suggest = Request::with_body(Method::Post, format!("{base}/search/api/v1/suggestions?scenario=powerbar"), body);
    suggest.headers = headers.clone();
    requests.push(("suggest", suggest));
    let qs = json!({"queryString": format!("NOT (isClientSoftDeleted:TRUE) AND {text}"), "displayQueryString": text});
    let plain = json!({"queryString": text, "displayQueryString": text});
    let qbody = json!({"EntityRequests":[
        {"entityType":"Message","contentSources":["Teams"],"propertySet":"Optimized","fields":["Extension_SkypeSpaces_ConversationPost_Extension_FromSkypeInternalId_String","Extension_SkypeSpaces_ConversationPost_Extension_ThreadType_String","Extension_SkypeSpaces_ConversationPost_Extension_ParentMessageId_String"],"query":qs,"from":0,"size":5,"topResultsCount":3},
        {"entityType":"People","contentSources":["Exchange"],"query":plain,"size":3},
        {"entityType":"Chat","contentSources":["Teams"],"query":plain,"size":3},
        {"entityType":"TeamsChannel","contentSources":["Teams"],"query":plain,"size":3},
        {"entityType":"File","contentSources":["OneDriveBusiness","Exchange"],"query":plain,"size":3}],
      "AnswerEntityRequests":[{"Query":{"QueryString":text},"EntityTypes":["Event"],"Size":5}],
      "QueryAlterationOptions":{"EnableAlteration":true,"EnableSuggestion":true,"SupportedRecourseDisplayTypes":["Suggestion"]},
      "scenario":{"Name":"powerbar","Dimensions":[{"DimensionName":"QueryType","DimensionValue":"All"},{"DimensionName":"FormFactor","DimensionValue":"general.web.reactSearch"}]},
      "cvid":cvid,"logicalId":uuid::Uuid::new_v4().to_string(),"timezone":"Europe/Berlin"});
    let mut query_request = Request::with_body(Method::Post, format!("{base}/searchservice/api/v2/query"), qbody);
    query_request.headers = headers.iter().map(|(k, v)| if k == "x-client-version" { (k.clone(), "1".to_owned()) } else { (k.clone(), v.clone()) }).collect();
    requests.push(("query", query_request));
    for (name, request) in requests {
        let answer = session.batch(&[request], &scope).await.map(|mut answers| answers.remove(0));
        match answer {
            Ok(answer) => {
                println!("{name}: HTTP {}", answer.status);
                std::fs::write(format!("{out}/{name}.json"), serde_json::to_string_pretty(&answer.body).unwrap()).unwrap();
            }
            Err(error) => println!("{name}: error {error}"),
        }
    }
}
