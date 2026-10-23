#![no_std]
use aidoku::{
	DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider, Manga, MangaPageResult,
	Page, PageContent, Result, Source,
	alloc::{String, Vec},
	helpers::uri::encode_uri_component,
	imports::net::Request,
	prelude::*,
};

mod models;

use models::*;

const BASE_URL: &str = "https://comicland.org";
const API_URL: &str = "https://api.comicland.org/api";
const PAGE_SIZE: usize = 20;

struct ComicLand;

/// The API turns away requests that do not come from the website.
fn api(url: &str) -> Result<Request> {
	Ok(Request::get(url)?
		.header("Referer", &format!("{BASE_URL}/"))
		.header("Origin", BASE_URL))
}

fn manga_list(url: &str) -> Result<MangaPageResult> {
	let (entries, has_next_page) = match api(url)?.json_owned::<ApiResponse<PageData>>()?.data {
		Some(data) => data.into_page(PAGE_SIZE),
		None => (Vec::new(), false),
	};
	Ok(MangaPageResult {
		entries,
		has_next_page,
	})
}

/// `endpoint` may already carry a query (`/comics?status=ongoing`).
fn paged(endpoint: &str, page: i32) -> String {
	let offset = (page - 1).max(0) as usize * PAGE_SIZE;
	let sep = if endpoint.contains('?') { '&' } else { '?' };
	format!("{API_URL}{endpoint}{sep}offset={offset}&limit={PAGE_SIZE}")
}

impl Source for ComicLand {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		if let Some(query) = query.filter(|q| !q.trim().is_empty()) {
			let endpoint = format!("/comic/search?q={}", encode_uri_component(query));
			return manga_list(&paged(&endpoint, page));
		}
		let endpoint = filters
			.into_iter()
			.find_map(|f| match f {
				FilterValue::Select { id, value } if id == "category" => Some(value),
				_ => None,
			})
			.unwrap_or_else(|| "/comics/popular".into());
		manga_list(&paged(&endpoint, page))
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		// one request returns both
		if needs_details || needs_chapters {
			let url = format!("{API_URL}/comic/detail?slug={}", manga.key);
			let detail = api(&url)?
				.json_owned::<ApiResponse<ComicDetailDto>>()?
				.data
				.ok_or_else(|| error!("Failed to parse manga details"))?;
			detail.apply(&mut manga, needs_chapters);
		}
		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: aidoku::Chapter) -> Result<Vec<Page>> {
		let url = format!(
			"{API_URL}/chapter/pages_by_index?slug={}&index={}",
			manga.key, chapter.key
		);
		let pages = api(&url)?
			.json_owned::<ApiResponse<PagesData>>()?
			.data
			.and_then(|d| d.pages)
			.unwrap_or_default();
		Ok(pages
			.into_iter()
			.map(|url| Page {
				content: PageContent::url(url),
				..Default::default()
			})
			.collect())
	}
}

impl ListingProvider for ComicLand {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let endpoint = match listing.id.as_str() {
			"Latest" => "/comics?status=ongoing",
			_ => "/comics/popular",
		};
		manga_list(&paged(endpoint, page))
	}
}

impl DeepLinkHandler for ComicLand {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url
			.strip_prefix(BASE_URL)
			.and_then(|p| p.strip_prefix("/comic/"))
		else {
			return Ok(None);
		};
		let path = path.split(['?', '#']).next().unwrap_or_default();
		let mut parts = path.split('/');
		let (Some(slug), chapter) = (parts.next().filter(|s| !s.is_empty()), parts.nth(1)) else {
			return Ok(None);
		};
		let manga_key = String::from(slug);
		Ok(Some(match chapter {
			Some(index) if !index.is_empty() => DeepLinkResult::Chapter {
				manga_key,
				key: index.into(),
			},
			_ => DeepLinkResult::Manga { key: manga_key },
		}))
	}
}

register_source!(ComicLand, ListingProvider, DeepLinkHandler);
