use aidoku::{
	Chapter, ContentRating, Manga, MangaStatus, Viewer,
	alloc::{String, Vec, string::ToString},
	prelude::*,
};
use serde::Deserialize;

use crate::BASE_URL;

// genres that mark a title as adult
const NSFW_GENRES: &[&str] = &[
	"Explicit Sex",
	"Sexual Content",
	"Borderline H",
	"Adult",
	"Hentai",
];

#[derive(Deserialize)]
pub struct ApiResponse<T> {
	pub data: Option<T>,
}

#[derive(Deserialize)]
pub struct PageData {
	list: Option<Vec<ComicDto>>,
	items: Option<Vec<ComicDto>>,
	has_more: Option<bool>,
}

impl PageData {
	pub fn into_page(self, page_size: usize) -> (Vec<Manga>, bool) {
		let comics = self.list.or(self.items).unwrap_or_default();
		let has_next_page = self.has_more.unwrap_or(comics.len() == page_size);
		(
			comics.into_iter().map(ComicDto::into_manga).collect(),
			has_next_page,
		)
	}
}

#[derive(Deserialize)]
pub struct ComicDto {
	slug: String,
	title: String,
	cover_url: Option<String>,
}

impl ComicDto {
	fn into_manga(self) -> Manga {
		Manga {
			url: Some(format!("{BASE_URL}/comic/{}", self.slug)),
			key: self.slug,
			title: self.title,
			cover: self.cover_url,
			..Default::default()
		}
	}
}

#[derive(Deserialize)]
struct NameDto {
	name: String,
}

#[derive(Deserialize)]
pub struct ComicDetailDto {
	slug: String,
	title: String,
	cover_url: Option<String>,
	description: Option<String>,
	authors: Option<Vec<NameDto>>,
	artists: Option<Vec<NameDto>>,
	genres: Option<Vec<NameDto>>,
	chapters: Option<Vec<ChapterDto>>,
}

impl ComicDetailDto {
	/// Fill in `manga` with the details; the chapters (newest first) when `with_chapters` is set.
	pub fn apply(self, manga: &mut Manga, with_chapters: bool) {
		let names = |list: Option<Vec<NameDto>>| {
			list.map(|l| l.into_iter().map(|n| n.name).collect::<Vec<String>>())
				.filter(|l| !l.is_empty())
		};
		let tags = names(self.genres);
		manga.content_rating = match &tags {
			Some(t) if t.iter().any(|g| NSFW_GENRES.contains(&g.as_str())) => ContentRating::NSFW,
			_ => ContentRating::Safe,
		};
		manga.url = Some(format!("{BASE_URL}/comic/{}", self.slug));
		manga.title = self.title;
		manga.cover = self.cover_url;
		manga.description = self.description;
		manga.authors = names(self.authors);
		manga.artists = names(self.artists);
		manga.tags = tags;
		manga.status = MangaStatus::Unknown;
		manga.viewer = Viewer::Webtoon;
		if with_chapters {
			let slug = self.slug;
			manga.chapters = Some(
				self.chapters
					.unwrap_or_default()
					.into_iter()
					.rev()
					.map(|c| c.into_chapter(&slug))
					.collect(),
			);
		}
	}
}

#[derive(Deserialize)]
struct ChapterDto {
	chapter_index: f32,
	title: String,
}

impl ChapterDto {
	fn into_chapter(self, slug: &str) -> Chapter {
		// whole numbers print without a decimal point: 13, 13.5
		let key = self.chapter_index.to_string();
		Chapter {
			url: Some(format!("{BASE_URL}/comic/{slug}/chapter/{key}")),
			key,
			title: Some(self.title),
			chapter_number: Some(self.chapter_index),
			..Default::default()
		}
	}
}

#[derive(Deserialize)]
pub struct PagesData {
	pub pages: Option<Vec<String>>,
}
