#![no_std]
use aidoku::{
	Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, HashMap, Home, HomeComponent,
	HomeComponentValue, HomeLayout, HomePartialResult, Link, Listing, ListingProvider, Manga,
	MangaPageResult, MangaStatus, Page, PageContent, Result, Source, Viewer, WebLoginHandler,
	alloc::{String, Vec},
	helpers::uri::encode_uri_component,
	imports::{
		html::{Document, Element},
		net::Request,
		std::{parse_date, send_partial_result},
	},
	prelude::*,
};

const BASE_URL: &str = "https://comichubfree.com";
const NEXT_PAGE: &str = "ul.pagination a[rel=next]";

struct ComicHubFree;

/// The cookie the site's browser check hands out once it is passed.
const CLEARANCE_COOKIE: &str = "cf_clearance";
const VERIFY_KEY: &str = "verify";

fn html(url: &str) -> Result<Document> {
	let response = Request::get(url)?
		.header("Referer", &format!("{BASE_URL}/"))
		.send()?;
	// the site sits behind a browser check: it has to be passed once, in the source's settings
	if matches!(response.status_code(), 403 | 503)
		&& response
			.get_header("cf-mitigated")
			.is_some_and(|value| value == "challenge")
	{
		bail!("Verification required in settings.");
	}
	Ok(response.get_html()?)
}

/// The image of a card or cover: lazy-loaded ones keep it in `data-src`.
fn image(element: &Element) -> Option<String> {
	element
		.attr("abs:data-src")
		.or_else(|| element.attr("abs:src"))
}

fn manga_list(url: &str) -> Result<MangaPageResult> {
	let html = html(url)?;
	let entries = html
		.select(".movie-list-index > .cartoon-box:has(.detail)")
		.map(|elements| {
			elements
				.filter_map(|element| {
					let link = element.select_first("a")?.attr("abs:href")?;
					Some(Manga {
						key: link.strip_prefix(BASE_URL)?.into(),
						title: element.select_first("h3")?.text()?,
						cover: element.select_first("img").and_then(|img| image(&img)),
						url: Some(link),
						viewer: Viewer::LeftToRight,
						..Default::default()
					})
				})
				.collect::<Vec<Manga>>()
		})
		.unwrap_or_default();
	let has_next_page = html.select_first(NEXT_PAGE).is_some();
	Ok(MangaPageResult {
		entries,
		has_next_page,
	})
}

fn chapters(mut html: Document) -> Result<Vec<Chapter>> {
	let mut chapters = Vec::new();
	loop {
		if let Some(rows) = html.select("div.episode-list > div > table > tbody > tr") {
			for row in rows {
				let Some(link) = row.select_first("a") else {
					continue;
				};
				let Some(url) = link.attr("abs:href") else {
					continue;
				};
				let title = link.text().unwrap_or_default();
				chapters.push(Chapter {
					key: url.strip_prefix(BASE_URL).unwrap_or(&url).into(),
					chapter_number: chapter_number(&title),
					date_uploaded: row
						.select("td:last-of-type")
						.and_then(|cells| cells.text())
						.and_then(|date| parse_date(date.trim(), "d-MMM-yyyy")),
					title: Some(title),
					url: Some(url),
					..Default::default()
				});
			}
		}
		let Some(next) = html
			.select_first(NEXT_PAGE)
			.and_then(|a| a.attr("abs:href"))
		else {
			break;
		};
		html = self::html(&next)?;
	}
	Ok(chapters)
}

/// The number after the last "#" (the site names chapters "Title #12").
fn chapter_number(title: &str) -> Option<f32> {
	let rest = title.rsplit_once('#')?.1;
	let number: String = rest
		.chars()
		.take_while(|c| c.is_ascii_digit() || *c == '.')
		.collect();
	number.parse().ok()
}

impl Source for ComicHubFree {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		// without a search the site has nothing to show for an empty key: browse the popular titles instead
		let Some(query) = query.filter(|q| !q.trim().is_empty()) else {
			return manga_list(&format!("{BASE_URL}/popular-comic?page={page}"));
		};
		let key = encode_uri_component(query);
		manga_list(&format!("{BASE_URL}/search-comic?key={key}&page={page}"))
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let html = html(&format!("{BASE_URL}{}", manga.key))?;

		if needs_details && let Some(info) = html.select_first("div.movie-info") {
			let series = info.select_first("div.series-info");
			manga.description = info.select_first("div#film-content").and_then(|e| e.text());
			if let Some(series) = &series {
				if let Some(cover) = series.select_first("img").and_then(|img| image(&img)) {
					manga.cover = Some(cover);
				}
				// the label reads "Author:" (the Mihon extension still looks for "Authors:")
				manga.authors = series
					.select("dt:contains(Author) + dd")
					.and_then(|e| e.text())
					.map(|a| aidoku::alloc::vec![a]);
				manga.status = match series
					.select("dt:contains(Status:) + dd")
					.and_then(|e| e.text())
					.as_deref()
				{
					Some("Ongoing") => MangaStatus::Ongoing,
					Some("Completed") => MangaStatus::Completed,
					_ => MangaStatus::Unknown,
				};
			}
			manga.tags = series.as_ref().and_then(|series| {
				series.select("dt:contains(Genres:) + dd a").map(|links| {
					links
						.filter_map(|link| link.text())
						.collect::<Vec<String>>()
				})
			});
			manga.url = Some(format!("{BASE_URL}{}", manga.key));
			manga.viewer = Viewer::LeftToRight;
		}

		if needs_chapters {
			manga.chapters = Some(chapters(html)?);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let html = html(&format!("{BASE_URL}{}/all", chapter.key))?;
		let mut seen = Vec::<String>::new();
		for img in html.select("img.chapter_img").into_iter().flatten() {
			if let Some(url) = image(&img)
				&& !seen.contains(&url)
			{
				seen.push(url);
			}
		}
		Ok(seen
			.into_iter()
			.map(|url| Page {
				content: PageContent::url(url),
				..Default::default()
			})
			.collect())
	}
}

impl ListingProvider for ComicHubFree {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let path = match listing.id.as_str() {
			"Latest" => "new-comic",
			_ => "popular-comic",
		};
		manga_list(&format!("{BASE_URL}/{path}?page={page}"))
	}
}

impl Home for ComicHubFree {
	fn get_home(&self) -> Result<HomeLayout> {
		for (title, path) in [("Popular", "popular-comic"), ("Latest", "new-comic")] {
			let entries = manga_list(&format!("{BASE_URL}/{path}?page=1"))?
				.entries
				.into_iter()
				.map(Link::from)
				.collect::<Vec<Link>>();
			if entries.is_empty() {
				continue;
			}
			send_partial_result(&HomePartialResult::Component(HomeComponent {
				title: Some(title.into()),
				value: HomeComponentValue::Scroller {
					entries,
					listing: Some(Listing {
						id: title.into(),
						name: title.into(),
						..Default::default()
					}),
				},
				..Default::default()
			}));
		}
		Ok(HomeLayout::default())
	}
}

impl DeepLinkHandler for ComicHubFree {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url
			.strip_prefix(BASE_URL)
			.filter(|p| p.starts_with("/comic/"))
		else {
			return Ok(None);
		};
		let path = path
			.split(['?', '#'])
			.next()
			.unwrap_or_default()
			.trim_end_matches('/');
		// /comic/<slug> is a title, /comic/<slug>/<chapter> a chapter
		Ok(Some(match path.matches('/').count() {
			2 => DeepLinkResult::Manga { key: path.into() },
			3.. => DeepLinkResult::Chapter {
				manga_key: path.rsplit_once('/').map(|(m, _)| m).unwrap_or(path).into(),
				key: path.into(),
			},
			_ => return Ok(None),
		}))
	}
}

impl WebLoginHandler for ComicHubFree {
	fn handle_web_login(&self, key: String, cookies: HashMap<String, String>) -> Result<bool> {
		Ok(key == VERIFY_KEY && cookies.contains_key(CLEARANCE_COOKIE))
	}
}

register_source!(
	ComicHubFree,
	ListingProvider,
	Home,
	DeepLinkHandler,
	WebLoginHandler
);
