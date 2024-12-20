use actix_files as fs; // 静的ファイルの提供に必要
use dotenv::dotenv;
use actix_web::{web, App, HttpServer, Responder, HttpResponse};
use actix_multipart::Multipart;
use actix_cors::Cors;
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use sanitize_filename::sanitize;
use log::{info, error};
use env_logger::Env;
use sqlx::sqlite::SqlitePoolOptions;

// 記事の構造体
#[derive(Serialize, Deserialize)]
struct Article {
    id: i64,
    title: Option<String>,
    content: Option<String>,
    is_draft: bool,
    thumbnail: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

// 記事一覧専用の構造体
#[derive(Serialize, Deserialize)]
struct ListArticle {
    id: i64,
    title: Option<String>,
    is_draft: bool,
    thumbnail: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

// 新規記事のリクエスト構造体
#[derive(Deserialize)]
struct CreateArticle {
    title: String,
    content: String,
    is_draft: bool,
    thumbnail: Option<String>,
}

// 記事更新のリクエスト構造体
#[derive(Deserialize)]
struct UpdateArticle {
    title: Option<String>,
    content: Option<String>,
    is_draft: Option<bool>,
    thumbnail: Option<String>,
}

// メイン関数
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    info!("Server is running at http://127.0.0.1:8080");

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("Failed to create pool.");

    sqlx::migrate!("./migrations").run(&pool).await.expect("Failed to run migrations");

    HttpServer::new(move || {
        App::new()
            .wrap(
                Cors::default()
                    .allow_any_origin()
                    .allow_any_method()
                    .allow_any_header(),
            )
            .app_data(web::Data::new(pool.clone()))
            .service(fs::Files::new("/uploads", "./uploads").show_files_listing())
            .service(
                web::resource("/api/articles/list")
                    .route(web::get().to(list_articles)),
            )
            .service(
                web::resource("/api/articles")
                    .route(web::post().to(create_article))
                    .route(web::get().to(get_articles)),
            )
            .service(
                web::resource("/api/articles/{id}")
                    .route(web::get().to(get_article))
                    .route(web::put().to(update_article))
                    .route(web::delete().to(delete_article)),
            )
            .service(
                web::resource("/api/upload-image")
                    .route(web::post().to(upload_image)),
            )
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

// 記事の作成エンドポイント
async fn create_article(pool: web::Data<sqlx::SqlitePool>, item: web::Json<CreateArticle>) -> impl Responder {
    let result = sqlx::query!(
        "INSERT INTO articles (title, content, is_draft, thumbnail) VALUES (?, ?, ?, ?)",
        item.title,
        item.content,
        item.is_draft,
        item.thumbnail
    )
    .execute(pool.get_ref())
    .await;

    match result {
        Ok(res) => HttpResponse::Created().json(res.last_insert_rowid()),
        Err(e) => {
            error!("Failed to create article: {}", e);
            HttpResponse::InternalServerError().body("Failed to create article")
        }
    }
}

// 記事一覧の取得エンドポイント
async fn get_articles(pool: web::Data<sqlx::SqlitePool>) -> impl Responder {
    let articles = sqlx::query_as!(
        Article,
        "SELECT id, title, content, is_draft, thumbnail, created_at, updated_at FROM articles ORDER BY created_at DESC"
    )
    .fetch_all(pool.get_ref())
    .await;

    match articles {
        Ok(articles) => HttpResponse::Ok().json(articles),
        Err(e) => {
            error!("Failed to get articles: {}", e);
            HttpResponse::InternalServerError().body("Failed to get articles")
        }
    }
}

// 記事一覧専用の取得エンドポイント
async fn list_articles(pool: web::Data<sqlx::SqlitePool>) -> impl Responder {
    let articles = sqlx::query_as!(
        ListArticle,
        "SELECT id, title, is_draft, thumbnail, created_at, updated_at FROM articles ORDER BY created_at DESC"
    )
    .fetch_all(pool.get_ref())
    .await;

    match articles {
        Ok(articles) => HttpResponse::Ok().json(articles),
        Err(e) => {
            error!("Failed to list articles: {}", e);
            HttpResponse::InternalServerError().body("Failed to list articles")
        }
    }
}

// 単一記事の取得エンドポイント
async fn get_article(pool: web::Data<sqlx::SqlitePool>, article_id: web::Path<i64>) -> impl Responder {
    let id = article_id.into_inner();
    let article = sqlx::query_as!(
        Article,
        "SELECT id, title, content, is_draft, thumbnail, created_at, updated_at FROM articles WHERE id = ?",
        id
    )
    .fetch_one(pool.get_ref())
    .await;

    match article {
        Ok(article) => HttpResponse::Ok().json(article),
        Err(e) => {
            error!("Failed to get article {}: {}", id, e);
            HttpResponse::NotFound().body("Article not found")
        }
    }
}

// 記事更新のエンドポイント
async fn update_article(
    pool: web::Data<sqlx::SqlitePool>,
    article_id: web::Path<i64>,
    item: web::Json<UpdateArticle>,
) -> impl Responder {
    let id = article_id.into_inner();

    let result = sqlx::query!(
        "UPDATE articles SET title = COALESCE(?, title), content = COALESCE(?, content), is_draft = COALESCE(?, is_draft), thumbnail = COALESCE(?, thumbnail), updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        item.title,
        item.content,
        item.is_draft,
        item.thumbnail,
        id
    )
    .execute(pool.get_ref())
    .await;

    match result {
        Ok(res) => {
            if res.rows_affected() == 0 {
                HttpResponse::NotFound().body("Article not found")
            } else {
                HttpResponse::Ok().body("Article updated successfully")
            }
        }
        Err(e) => {
            error!("Failed to update article {}: {}", id, e);
            HttpResponse::InternalServerError().body("Failed to update article")
        }
    }
}

// 記事の削除エンドポイント
async fn delete_article(pool: web::Data<sqlx::SqlitePool>, article_id: web::Path<i64>) -> impl Responder {
    let id = article_id.into_inner();
    let result = sqlx::query!(
        "DELETE FROM articles WHERE id = ?",
        id
    )
    .execute(pool.get_ref())
    .await;

    match result {
        Ok(res) => {
            if res.rows_affected() == 0 {
                HttpResponse::NotFound().body("Article not found")
            } else {
                HttpResponse::Ok().body("Article deleted successfully")
            }
        }
        Err(e) => {
            error!("Failed to delete article {}: {}", id, e);
            HttpResponse::InternalServerError().body("Failed to delete article")
        }
    }
}

// 画像アップロードエンドポイント
async fn upload_image(mut payload: Multipart) -> impl Responder {
    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(f) => f,
            Err(e) => {
                error!("Error processing field: {}", e);
                return HttpResponse::BadRequest().body("Error processing field");
            }
        };

        let content_disposition = field.content_disposition();

        if let Some(filename) = content_disposition.get_filename() {
            let sanitized_filename = sanitize(&filename);
            let filepath = format!("./uploads/{}", sanitized_filename);
            info!("Uploading image: {}", filepath);

            if let Err(e) = tokio::fs::create_dir_all("./uploads").await {
                error!("Failed to create uploads directory: {}", e);
                return HttpResponse::InternalServerError().body("Error creating uploads directory");
            }

            let mut f = match File::create(&filepath).await {
                Ok(file) => file,
                Err(e) => {
                    error!("Failed to create file {}: {}", filepath, e);
                    return HttpResponse::InternalServerError().body("Error creating file");
                }
            };

            while let Some(chunk) = field.next().await {
                let data = match chunk {
                    Ok(d) => d,
                    Err(e) => {
                        error!("Error reading chunk: {}", e);
                        return HttpResponse::BadRequest().body("Error reading chunk");
                    }
                };

                if let Err(e) = f.write_all(&data).await {
                    error!("Error writing to file {}: {}", filepath, e);
                    return HttpResponse::InternalServerError().body("Error writing to file");
                }
            }

            info!("Successfully uploaded image: {}", filepath);

            let image_url = format!("http://127.0.0.1:8080/uploads/{}", sanitized_filename);
            return HttpResponse::Ok().body(image_url);
        } else {
            error!("Filename not found in Content-Disposition");
            return HttpResponse::BadRequest().body("Filename not found in Content-Disposition");
        }
    }

    HttpResponse::BadRequest().body("No file uploaded")
}
