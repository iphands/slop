#!/usr/bin/env python3
"""Dump all topics and posts from Qhimm.com Forums.

Scrapes an SMF board listing all topics, then fetches every post in each topic.
Outputs per-topic JSON files plus a combined full dump.

Usage:
    python dump_forum.py                          # Default: board 4
    python dump_forum.py --board 7 --output out   # Board 7, custom output dir
    python dump_forum.py --resume                 # Skip already-dumped topics
    python dump_forum.py --refresh                # Re-scrape topic index
    python dump_forum.py --text-only              # Skip HTML storage
"""

import argparse
import gzip
import json
import re
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import requests
from bs4 import BeautifulSoup


BASE_URL = "https://forums.qhimm.com/index.php"
TOPICS_PER_PAGE = 20
POSTS_PER_PAGE = 25
DEFAULT_DELAY = 0.5
DEFAULT_BOARD = 4
MAX_RETRIES = 3
SESSION_COOLDOWN = 10

HEADERS = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
    "Accept-Language": "en-US,en;q=0.9",
    "Accept-Encoding": "gzip, deflate, br",
    "Connection": "keep-alive",
    "Upgrade-Insecure-Requests": "1",
}


class SessionStaleError(Exception):
    pass


def sanitize_filename(name: str) -> str:
    name = re.sub(r'[<>:"/\\|?*]', '_', name)
    name = re.sub(r'\s+', '_', name)
    return name[:80]


def create_session() -> requests.Session:
    session = requests.Session()
    session.headers.update(HEADERS)
    return session


def is_challenge_page(html: str) -> bool:
    soup = BeautifulSoup(html, "html.parser")
    has_forum_structure = bool(soup.select_one("#forumposts, #messageindex"))
    if not has_forum_structure:
        return True
    title = (soup.title.string or "").lower() if soup.title else ""
    challenge_keywords = ["captcha", "challenge", "verify you are human", "access denied", "rate limit"]
    return any(p in title for p in challenge_keywords)


def fetch(session: requests.Session, url: str) -> str:
    for attempt in range(MAX_RETRIES):
        try:
            resp = session.get(url, timeout=10)
            resp.raise_for_status()
            if is_challenge_page(resp.text):
                raise SessionStaleError(f"Challenge page detected on {url}")
            return resp.text
        except requests.HTTPError as e:
            status = e.response.status_code if e.response is not None else None
            if status in (429, 503) and attempt == MAX_RETRIES - 1:
                raise SessionStaleError from e
            if status and status >= 429 and attempt < MAX_RETRIES - 1:
                wait = 2 ** (attempt + 1)
                print(f"    Server error {status}, retrying in {wait}s...")
                time.sleep(wait)
                continue
            raise
        except (requests.ConnectionError, requests.Timeout):
            if attempt < MAX_RETRIES - 1:
                wait = 2 ** (attempt + 1)
                print(f"    Connection error, retrying in {wait}s...")
                time.sleep(wait)
                continue
            raise


def parse_topic_id_from_url(url: str) -> int:
    m = re.search(r'topic=(\d+)', url)
    if not m:
        print(f"    WARNING: No topic ID found in URL: {url}")
        return 0
    return int(m.group(1))


def get_total_board_pages(html: str) -> int:
    soup = BeautifulSoup(html, "html.parser")
    pagelinks = soup.select(".pagelinks a.navPages")
    if not pagelinks:
        return 1
    last_link = pagelinks[-1].get("href", "")
    m = re.search(r'board=\d+\.(\d+)', last_link)
    if m:
        last_start = int(m.group(1))
        return (last_start // TOPICS_PER_PAGE) + 1
    return 1


def scrape_board_topics(session: requests.Session, board_id: int, delay: float) -> list[dict]:
    topics = []

    url = f"{BASE_URL}?board={board_id}.0"
    print(f"Fetching board page 1: {url}")
    html = fetch(session, url)
    total_pages = get_total_board_pages(html)
    print(f"Total board pages: {total_pages}")

    for page in range(total_pages):
        start = page * TOPICS_PER_PAGE
        url = f"{BASE_URL}?board={board_id}.{start}"
        print(f"  [{page + 1}/{total_pages}] Board page (start={start})")

        if page == 0:
            pass
        else:
            html = fetch(session, url)

        soup = BeautifulSoup(html, "html.parser")

        for row in soup.select("#messageindex tbody tr"):
            classes = " ".join(row.get("class", []))
            if "stickybg" in classes:
                continue

            subject_cell = row.select_one(".subject")
            if not subject_cell:
                continue

            topic_link = subject_cell.select_one("a")
            if not topic_link:
                continue

            topic_id = parse_topic_id_from_url(topic_link.get("href", ""))
            if topic_id == 0:
                continue

            subject = topic_link.get_text(strip=True)

            starter = subject_cell.select_one("p a")
            starter_name = starter.get_text(strip=True) if starter else ""

            stats = row.select_one(".stats")
            replies_text = stats.get_text(strip=True) if stats else ""
            replies_match = re.search(r'(\d+)\s*Replies?', replies_text)
            replies = int(replies_match.group(1)) if replies_match else 0

            estimated_pages = max(1, (replies // POSTS_PER_PAGE) + 1)

            topics.append({
                "topic_id": topic_id,
                "subject": subject,
                "starter": starter_name,
                "replies": replies,
                "estimated_pages": estimated_pages,
                "url": f"{BASE_URL}?topic={topic_id}.0",
                "posts": [],
            })

        if page < total_pages - 1:
            time.sleep(delay)

    return topics


def scrape_topic_posts(session: requests.Session, topic: dict, delay: float, text_only: bool) -> None:
    topic_id = topic["topic_id"]
    page = 0

    while True:
        start = page * POSTS_PER_PAGE
        url = f"{BASE_URL}?topic={topic_id}.{start}"

        html = fetch(session, url)
        soup = BeautifulSoup(html, "html.parser")

        post_wrappers = soup.select("#forumposts .post_wrapper")
        if not post_wrappers:
            break

        for post_wrapper in post_wrappers:
            author_el = post_wrapper.select_one(".poster h4 a")
            author = author_el.get_text(strip=True) if author_el else ""

            subject_el = post_wrapper.select_one(".keyinfo h5 a")
            post_subject = subject_el.get_text(strip=True) if subject_el else ""

            date_el = post_wrapper.select_one(".keyinfo .smalltext")
            date_text = date_el.get_text(strip=True) if date_el else ""
            date_match = re.search(r'(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2})', date_text)
            date_str = date_match.group(1) if date_match else ""

            body_el = post_wrapper.select_one(".post .inner")
            body_text = body_el.get_text(separator="\n", strip=True) if body_el else ""
            body_html = body_el.prettify() if body_el and not text_only else ""

            reply_match = re.search(r'Reply #(\d+)', date_text)
            reply_num = int(reply_match.group(1)) if reply_match else 0

            topic["posts"].append({
                "author": author,
                "date": date_str,
                "subject": post_subject,
                "reply_num": reply_num,
                "body_text": body_text,
                "body_html": body_html,
            })

        bottom_pagelinks = soup.select("#forumposts ~ .pagesection .pagelinks a.navPages")
        if not bottom_pagelinks:
            break

        page += 1
        time.sleep(delay)


def main():
    parser = argparse.ArgumentParser(description="Dump Qhimm.com forum topics and posts")
    parser.add_argument("--board", type=int, default=DEFAULT_BOARD, help=f"Board ID to scrape (default: {DEFAULT_BOARD})")
    parser.add_argument("--output", type=str, default="dump", help="Output directory (default: dump)")
    parser.add_argument("--delay", type=float, default=DEFAULT_DELAY, help=f"Seconds between requests (default: {DEFAULT_DELAY})")
    parser.add_argument("--text-only", action="store_true", help="Skip HTML storage (saves space)")
    parser.add_argument("--resume", action="store_true", help="Skip topics that already have dump files")
    parser.add_argument("--refresh", action="store_true", help="Re-scrape topic index even if cached")
    args = parser.parse_args()

    output_dir = Path(args.output)
    output_dir.mkdir(exist_ok=True)

    session = create_session()

    print("=" * 60)
    print(f"Qhimm Forums Dump - Board {args.board}")
    if args.text_only:
        print("Mode: text-only (no HTML storage)")
    print(f"Output: {output_dir.absolute()}")
    print(f"Delay: {args.delay}s between requests")
    print("=" * 60)

    index_file = output_dir / "topics_index.json"

    if index_file.exists() and not args.refresh:
        print("\n--- Phase 1: Loading cached topic index ---")
        try:
            with open(index_file, "r", encoding="utf-8") as f:
                cached = json.load(f)

            if isinstance(cached, list):
                cached_topics = cached
                cached_board = None
                cached_at = None
                print("  Note: Old-format index detected. Consider running --refresh for a clean index.")
            else:
                cached_topics = cached.get("topics", [])
                cached_board = cached.get("board_id")
                cached_at = cached.get("scraped_at")

            if cached_board is not None and cached_board != args.board:
                print(f"  WARNING: Cache is for board {cached_board}, but you requested board {args.board}. "
                      f"Re-scraping...")
                raise ValueError("board mismatch")

            topics = [
                {
                    "topic_id": t["topic_id"],
                    "subject": t["subject"],
                    "starter": t["starter"],
                    "replies": t["replies"],
                    "estimated_pages": max(1, (t["replies"] // POSTS_PER_PAGE) + 1),
                    "url": t["url"],
                    "posts": [],
                }
                for t in cached_topics
            ]

            if cached_at:
                cached_dt = datetime.fromisoformat(cached_at.replace("Z", "+00:00"))
                age_hours = (time.time() - cached_dt.timestamp()) / 3600
                print(f"  Loaded {len(topics)} topics from cache ({age_hours:.1f}h old). Use --refresh to re-scrape.")
            else:
                print(f"  Loaded {len(topics)} topics from cache.")

        except (json.JSONDecodeError, KeyError, ValueError) as e:
            print(f"  Cache issue ({e}). Re-scraping...")
            try:
                topics = scrape_board_topics(session, args.board, args.delay)
            except SessionStaleError:
                print(f"  Session stale during board scrape. Cooling down and rotating...")
                time.sleep(SESSION_COOLDOWN)
                session = create_session()
                session.get(BASE_URL, timeout=10)
                topics = scrape_board_topics(session, args.board, args.delay)
    else:
        print("\n--- Phase 1: Collecting topics ---")
        try:
            topics = scrape_board_topics(session, args.board, args.delay)
        except SessionStaleError:
            print(f"  Session stale during board scrape. Cooling down and rotating...")
            time.sleep(SESSION_COOLDOWN)
            session = create_session()
            session.get(BASE_URL, timeout=10)
            topics = scrape_board_topics(session, args.board, args.delay)

    print(f"\nFound {len(topics)} topics total.")

    if not topics:
        print("No topics found. Exiting.")
        sys.exit(0)

    topic_index = {
        "board_id": args.board,
        "scraped_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "topic_count": len(topics),
        "topics": [
            {
                "topic_id": t["topic_id"],
                "subject": t["subject"],
                "starter": t["starter"],
                "replies": t["replies"],
                "url": t["url"],
            }
            for t in topics
        ],
    }
    with open(index_file, "w", encoding="utf-8") as f:
        json.dump(topic_index, f, indent=2, ensure_ascii=False)
    print(f"Topic index saved to {index_file}")

    print(f"\n--- Phase 2: Scraping posts ({len(topics)} topics) ---")
    failed_topics = []
    skipped_topics = 0

    for i, topic in enumerate(topics):
        topic_file = output_dir / f"topic_{topic['topic_id']:05d}_{sanitize_filename(topic['subject'])}.json"
        if args.resume and topic_file.exists():
            with open(topic_file, "r", encoding="utf-8") as f:
                existing = json.load(f)
            if existing.get("posts"):
                skipped_topics += 1
                print(f"\n[{i + 1}/{len(topics)}] SKIPPED (exists): Topic {topic['topic_id']}")
                continue
            else:
                print(f"\n[{i + 1}/{len(topics)}] RETRYING (0 posts in cache): Topic {topic['topic_id']}")

        print(f"\n[{i + 1}/{len(topics)}] Topic {topic['topic_id']}: {topic['subject']}")
        print(f"  Replies: {topic['replies']}, Est. pages: {topic['estimated_pages']}")

        try:
            scrape_topic_posts(session, topic, args.delay, args.text_only)
            if not topic["posts"]:
                print(f"  ERROR: 0 posts scraped (likely blocked/challenged)")
                failed_topics.append({"topic_id": topic["topic_id"], "subject": topic["subject"], "error": "0 posts scraped"})
            else:
                print(f"  Scraped {len(topic['posts'])} posts")
                with open(topic_file, "w", encoding="utf-8") as f:
                    json.dump(topic, f, indent=2, ensure_ascii=False)
        except SessionStaleError:
            print(f"  Session stale, rotating and retrying once...")
            time.sleep(SESSION_COOLDOWN)
            session = create_session()
            session.get(BASE_URL, timeout=10)
            try:
                scrape_topic_posts(session, topic, args.delay, args.text_only)
                if not topic["posts"]:
                    print(f"  ERROR: 0 posts scraped after rotation (likely blocked/challenged)")
                    failed_topics.append({"topic_id": topic["topic_id"], "subject": topic["subject"], "error": "0 posts scraped after rotation"})
                else:
                    print(f"  Scraped {len(topic['posts'])} posts (after rotation)")
                    with open(topic_file, "w", encoding="utf-8") as f:
                        json.dump(topic, f, indent=2, ensure_ascii=False)
            except Exception as e:
                print(f"  ERROR: Failed after session rotation: {e}")
                failed_topics.append({"topic_id": topic["topic_id"], "subject": topic["subject"], "error": str(e)})
        except Exception as e:
            print(f"  ERROR: Failed to scrape topic {topic['topic_id']}: {e}")
            failed_topics.append({"topic_id": topic["topic_id"], "subject": topic["subject"], "error": str(e)})

        time.sleep(args.delay)

    print(f"\n--- Phase 3: Saving combined dump ---")

    combined = []
    for topic in topics:
        if topic["posts"]:
            combined.append(topic)
        elif args.resume:
            tf = output_dir / f"topic_{topic['topic_id']:05d}_{sanitize_filename(topic['subject'])}.json"
            if tf.exists():
                with open(tf, "r", encoding="utf-8") as f:
                    combined.append(json.load(f))

    with gzip.open(output_dir / "full_dump.json.gz", "wt", encoding="utf-8") as f:
        json.dump(combined, f, indent=2, ensure_ascii=False)

    total_posts = sum(len(t["posts"]) for t in combined)
    print(f"\n{'=' * 60}")
    print(f"Done! Dumped {len(combined)} topics, {total_posts} posts total.")
    if skipped_topics:
        print(f"Skipped (resume): {skipped_topics}")
    if failed_topics:
        print(f"FAILED: {len(failed_topics)} topics")
        for ft in failed_topics:
            print(f"  - {ft['topic_id']}: {ft['subject']} ({ft['error']})")
    print(f"Output directory: {output_dir.absolute()}")
    print(f"{'=' * 60}")


if __name__ == "__main__":
    main()
