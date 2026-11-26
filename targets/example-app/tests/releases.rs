//! A screen changed on purpose (package 9.1): each change of the form of a
//! line item is what its module says, changes nothing stored, and lasts
//! until a reset.

use std::error::Error;

use example_app::harness::TestApp;
use example_app::releases::{ChangedPage, ScreenChange};
use reqwest::StatusCode;

type Outcome = Result<(), Box<dyn Error>>;

const FORM: &str = "/orders/A-100/items/L-101";

async fn form(app: &TestApp) -> Result<String, Box<dyn Error>> {
    let response = app.http().get(app.url(FORM)).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(response.text().await?)
}

/// The part of `page` between the form's start and its end.
fn inside_form(page: &str) -> &str {
    let start = page.find("<form").unwrap_or(0);
    let end = page.find("</form>").unwrap_or(page.len());
    &page[start..end]
}

fn at(page: &str, text: &str) -> usize {
    page.find(text)
        .unwrap_or_else(|| panic!("{text} is not in {page}"))
}

#[tokio::test]
async fn each_change_is_what_a_release_makes_and_a_reset_undoes_it() -> Outcome {
    let app = TestApp::start().await?;
    let first = form(&app).await?;
    assert!(inside_form(&first).contains("<h2>Line Item Details</h2>"));
    assert!(first.contains(">Quantity</label>") && first.contains(">Save</button>"));
    assert!(!first.contains("class="));

    // Reword: one label and the button say other words; nothing else moves.
    app.change_screen(ChangedPage::LineItem, ScreenChange::Reword)
        .await?;
    let reworded = form(&app).await?;
    assert!(reworded.contains("<label for=\"quantity\">Qty</label>"));
    assert!(reworded.contains(">Save line item</button>"));
    assert!(!reworded.contains(">Quantity<") && !reworded.contains(">Save<"));
    assert_eq!(
        reworded
            .replace(">Qty<", ">Quantity<")
            .replace(">Save line item<", ">Save<"),
        first
    );

    // Restyle: class names and a style sheet, every identifier renamed, and
    // each label still names its field; the script finds the new ones.
    app.change_screen(ChangedPage::LineItem, ScreenChange::Restyle)
        .await?;
    let restyled = form(&app).await?;
    assert!(restyled.contains("<style>") && restyled.contains("class=\"ef-form\""));
    for field in ["item_code", "description", "quantity", "length"] {
        let renamed = format!("li-{field}-7f3a");
        assert!(restyled.contains(&format!("<label for=\"{renamed}\">")));
        assert!(restyled.contains(&format!("<input id=\"{renamed}\"")));
        assert!(!restyled.contains(&format!("id=\"{field}\"")));
    }
    assert!(restyled.contains("getElementById(\"li-line-item-7f3a\")"));
    assert!(restyled.contains("getElementById(\"li-status-7f3a\")"));
    assert!(!restyled.contains("\"line-item\""));
    assert!(restyled.contains(">Quantity</label>") && restyled.contains("Line Item Details"));

    // Reorder: the dimensions, the quantity, then the description.
    app.change_screen(ChangedPage::LineItem, ScreenChange::Reorder)
        .await?;
    let reordered = form(&app).await?;
    let (dimensions, quantity, description) = (
        at(&reordered, "<legend>Dimensions</legend>"),
        at(&reordered, ">Quantity</label>"),
        at(&reordered, ">Description</label>"),
    );
    assert!(dimensions < quantity && quantity < description);
    assert!(at(&first, ">Description</label>") < at(&first, "<legend>Dimensions</legend>"));
    assert_eq!(reordered.len(), first.len());

    // Wrap: the form sits in a card whose header takes its heading.
    app.change_screen(ChangedPage::LineItem, ScreenChange::Wrap)
        .await?;
    let wrapped = form(&app).await?;
    assert!(!inside_form(&wrapped).contains("Line Item Details"));
    let card = at(
        &wrapped,
        "<section class=\"card\" aria-labelledby=\"card-title\">",
    );
    assert!(card < at(&wrapped, "<h2 id=\"card-title\">Line Item Details</h2>"));
    assert!(at(&wrapped, "</h2>") < at(&wrapped, "<form"));
    assert!(at(&wrapped, "</form>") < at(&wrapped, "</section>"));

    // Relabel: two labels say other words at once; nothing else moves.
    app.change_screen(ChangedPage::LineItem, ScreenChange::Relabel)
        .await?;
    let relabelled = form(&app).await?;
    assert!(relabelled.contains("<label for=\"description\">Item</label>"));
    assert!(relabelled.contains("<label for=\"quantity\">Qty</label>"));
    assert_eq!(
        relabelled
            .replace(">Item<", ">Description<")
            .replace(">Qty<", ">Quantity<"),
        first
    );
    app.change_screen(ChangedPage::LineItem, ScreenChange::Wrap)
        .await?;

    // The control interface says what the page shows now.
    let reported: serde_json::Value = app
        .http()
        .get(app.url("/__control/screens"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(
        reported,
        serde_json::json!({"screens": [{"page": "line_item", "change": "wrap"}]})
    );

    // `none` and a reset each give the page as it was first released.
    app.change_screen(ChangedPage::LineItem, ScreenChange::None)
        .await?;
    assert_eq!(form(&app).await?, first);
    app.change_screen(ChangedPage::LineItem, ScreenChange::Reword)
        .await?;
    app.reset().await?;
    assert_eq!(form(&app).await?, first);

    // A change no rule names, or a page that cannot be changed, is refused.
    for body in [
        serde_json::json!({"page": "line_item", "change": "upgrade"}),
        serde_json::json!({"page": "order", "change": "reword"}),
    ] {
        let refused = app
            .http()
            .post(app.url("/__control/screens"))
            .json(&body)
            .send()
            .await?;
        assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert_eq!(form(&app).await?, first);
    assert!(app.writes().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn a_changed_form_keeps_every_name_and_role_its_script_reads() -> Outcome {
    let app = TestApp::start().await?;
    for change in [
        ScreenChange::Reword,
        ScreenChange::Restyle,
        ScreenChange::Reorder,
        ScreenChange::Wrap,
        ScreenChange::Relabel,
    ] {
        app.change_screen(ChangedPage::LineItem, change).await?;
        let page = form(&app).await?;
        // Every field the script reads is there by its name, and the script
        // finds the form, its status, and its problems.
        for name in ["item_code", "description", "quantity", "length_in"] {
            assert!(page.contains(&format!("name=\"{name}\"")), "{change:?}");
        }
        for role in ["role=\"status\"", "role=\"alert\""] {
            assert!(inside_form(&page).contains(role), "{change:?}");
        }
    }
    assert!(app.writes().await?.is_empty());
    assert!(app.stored_item("A-100", "L-101").await?.is_some());
    Ok(())
}
