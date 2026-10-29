use super::*;
#[derive(Deserialize)]
pub struct MediaToken {
    pub token: String,
}
/// Short-lived capability URL for networks which fetch media. It is not a public library.
pub async fn media_file(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<MediaToken>,
) -> Result<Response> {
    if query.token.len() != 64 {
        return Err(ApiError::missing());
    }
    let row=sqlx::query("SELECT m.storage_key,m.mime FROM provider_media_tokens t JOIN media m ON m.id=t.media_id WHERE t.token_hash=$1 AND t.media_id=$2 AND t.expires_at>now()").bind(crypto::digest(&query.token)).bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
    let key: String = row.get("storage_key");
    if key.contains('/') || key.contains('\\') {
        return Err(ApiError::missing());
    }
    let file = tokio::fs::File::open(state.config.media_dir.join(key))
        .await
        .map_err(|_| ApiError::missing())?;
    let size = file
        .metadata()
        .await
        .map_err(|_| ApiError::missing())?
        .len();
    let mut response = (
        StatusCode::OK,
        Body::from_stream(tokio_util::io::ReaderStream::new(file)),
    )
        .into_response();
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        size.to_string().parse().map_err(ApiError::internal)?,
    );
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        row.get::<String, _>("mime")
            .parse()
            .map_err(ApiError::internal)?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    Ok(response)
}
