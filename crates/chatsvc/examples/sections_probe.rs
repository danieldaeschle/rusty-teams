use chatsvc::{Folders, Pins};
use session::{DEFAULT_ENDPOINT, Session};

const TEST_NAME: &str = "zz rusty test";
const RENAMED: &str = "zz rusty test 2";

fn short(error: impl std::fmt::Display) -> String {
    error.to_string().chars().take(160).collect()
}

fn test_folder_id(folders: &Folders, name: &str) -> Option<String> {
    folders
        .folders
        .iter()
        .find(|folder| folder.name == name)
        .map(|folder| folder.id.clone())
}

fn position(folders: &Folders, folder_id: &str) -> Option<usize> {
    folders.order.iter().position(|id| id == folder_id)
}

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let pins = Pins::new(&session);
    let before = pins.folders().await.expect("folders");
    println!("# before: {} folders", before.folders.len());

    let created = match pins.create_folder(TEST_NAME).await {
        Ok(folders) => folders,
        Err(error) => return println!("# create failed: {}", short(error)),
    };
    let Some(folder_id) = test_folder_id(&created, TEST_NAME) else {
        return println!("# create ok but '{TEST_NAME}' not in the answer");
    };
    println!(
        "# create ok: {} folders, test section at order {:?}",
        created.folders.len(),
        position(&created, &folder_id)
    );

    match pins.rename_folder(&folder_id, RENAMED).await {
        Ok(folders) => println!(
            "# rename ok: name now '{}'",
            test_folder_id(&folders, RENAMED).map_or("missing", |_| RENAMED)
        ),
        Err(error) => println!("# rename failed: {}", short(error)),
    }

    let current = pins.folders().await.expect("folders");
    let previous = current
        .folders
        .iter()
        .map(|folder| folder.id.as_str())
        .take_while(|id| *id != folder_id)
        .last()
        .map(str::to_owned);
    match previous {
        Some(target) => match pins.move_folder(&folder_id, &target).await {
            Ok(folders) => println!(
                "# move up ok: order {:?} -> {:?}",
                position(&current, &folder_id),
                position(&folders, &folder_id)
            ),
            Err(error) => println!("# move up failed: {}", short(error)),
        },
        None => println!("# move up skipped: test section is first"),
    }

    match pins.delete_folder(&folder_id).await {
        Ok(folders) => println!(
            "# delete ok: {} folders, test section gone: {}",
            folders.folders.len(),
            !folders.folders.iter().any(|folder| folder.id == folder_id)
        ),
        Err(error) => println!("# delete failed: {}", short(error)),
    }
    let after = pins.folders().await.expect("folders");
    println!("# after: {} folders", after.folders.len());
}
