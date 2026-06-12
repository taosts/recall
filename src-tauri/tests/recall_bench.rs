//! Recall Quality Benchmark
//!
//! A fixed set of fuzzy queries against a synthetic browsing dataset.
//! Run with:
//!
//!   cargo test bench_recall_quality -- --nocapture

use recall_app_lib::db;
use recall_app_lib::normalizer;
use recall_app_lib::search;
use recall_app_lib::segmenter::Segmenter;
use rusqlite::{params, Connection};

struct BenchQuery {
    query: &'static str,
    expected_top5: &'static [&'static str],
    scenario: &'static str,
}

const BENCH_QUERIES: &[BenchQuery] = &[
    // Exact regression for the unicode61 bug: "驾考" is embedded mid-token in the
    // relevant titles; before the jieba-segmented search_text fix the literal
    // layer returned 0 and platform-word noise dominated.
    BenchQuery {
        query: "驾考",
        expected_top5: &["driving-zhihu", "driving-search", "driving-gov"],
        scenario: "精确中文词，嵌在长标题中（unicode61 回归测试）",
    },
    BenchQuery {
        query: "考驾照",
        expected_top5: &["driving-zhihu", "driving-gov", "driving-search"],
        scenario: "用户记得'考驾照'但库里是'驾考'",
    },
    BenchQuery {
        query: "驾照题库",
        expected_top5: &["driving-zhihu", "driving-search"],
        scenario: "部分词匹配",
    },
    BenchQuery {
        query: "科目一怎么考",
        expected_top5: &["driving-zhihu", "driving-bilibili"],
        scenario: "同义词扩展: 科目一 -> 驾考",
    },
    BenchQuery {
        query: "NAS 缓存文章",
        expected_top5: &["zfs-reddit", "zfs-archwiki"],
        scenario: "NAS -> ZFS 概念扩展",
    },
    BenchQuery {
        query: "ZFS ARC tuning",
        expected_top5: &["zfs-reddit", "zfs-archwiki"],
        scenario: "精确英文技术术语",
    },
    BenchQuery {
        query: "硬盘存储池配置",
        expected_top5: &["zfs-reddit", "zfs-archwiki", "zfs-truenas"],
        scenario: "存储 -> ZFS/NAS 概念链",
    },
    BenchQuery {
        query: "GitHub Actions caching tutorial",
        expected_top5: &["gha-devto", "gha-docs"],
        scenario: "精确英文描述",
    },
    BenchQuery {
        query: "CI cache optimization",
        expected_top5: &["gha-devto", "gha-docs"],
        scenario: "换词描述同一概念",
    },
    BenchQuery {
        query: "磁盘满了怎么解决",
        expected_top5: &["disk-csdn", "disk-ms-support"],
        scenario: "口语化描述 vs 技术标题",
    },
    BenchQuery {
        query: "disk 100% usage windows",
        expected_top5: &["disk-csdn", "disk-ms-support"],
        scenario: "英文描述中文内容",
    },
    BenchQuery {
        query: "旁路由 DNS 分流",
        expected_top5: &["openwrt-v2ex", "openwrt-official"],
        scenario: "精确中文技术描述",
    },
    BenchQuery {
        query: "软路由怎么设置",
        expected_top5: &["openwrt-v2ex", "openwrt-official"],
        scenario: "软路由 -> OpenWrt 同义词",
    },
    BenchQuery {
        query: "Rust borrow checker",
        expected_top5: &["rust-book-borrow"],
        scenario: "书签应排在前面",
    },
    BenchQuery {
        query: "知乎 驾考",
        expected_top5: &["driving-zhihu"],
        scenario: "登录/SSO 页不应出现在 top 5",
    },
    BenchQuery {
        query: "去年看的 Docker 教程",
        expected_top5: &["docker-tutorial"],
        scenario: "时间描述 + 内容描述",
    },
];

fn create_bench_db() -> Connection {
    let db_path = std::env::temp_dir().join(format!("recall-bench-{}.db", uuid::Uuid::new_v4()));
    db::init_db(&db_path).unwrap()
}

fn insert(
    conn: &Connection,
    id: &str,
    title: &str,
    url: &str,
    domain: &str,
    visited_at: &str,
    is_bookmarked: bool,
    visit_count: i64,
) {
    conn.execute(
        r#"INSERT OR IGNORE INTO artifacts
               (id, type, title, url, domain, created_at, visited_at,
                is_bookmarked, visit_count, source, embedding_version)
           VALUES (?1, 'history', ?2, ?3, ?4, ?5, ?5, ?6, ?7, 'edge', 0)"#,
        params![
            id,
            title,
            url,
            domain,
            visited_at,
            if is_bookmarked { 1 } else { 0 },
            visit_count
        ],
    )
    .unwrap();
}

fn create_bench_dataset(conn: &Connection) {
    insert(
        conn,
        "driving-search",
        "驾考宝典的题库哪里来的 - 搜索",
        "https://cn.bing.com/search?q=驾考宝典的题库哪里来的",
        "cn.bing.com",
        "2025-06-03T10:00:00",
        false,
        1,
    );
    insert(
        conn,
        "driving-zhihu",
        "驾考宝典官方题库来源分析 - 知乎",
        "https://www.zhihu.com/question/123456",
        "www.zhihu.com",
        "2025-06-03T10:05:00",
        true,
        3,
    );
    insert(
        conn,
        "driving-gov",
        "机动车驾驶证申领和使用规定 - 公安部",
        "https://www.gov.cn/driving-license-rules",
        "www.gov.cn",
        "2025-06-03T10:12:00",
        true,
        2,
    );
    insert(
        conn,
        "driving-bilibili",
        "科目一快速记忆技巧合集 - bilibili",
        "https://www.bilibili.com/video/BV1234",
        "www.bilibili.com",
        "2025-06-03T10:18:00",
        false,
        1,
    );
    insert(
        conn,
        "driving-sso",
        "统一认证登录 - 知乎",
        "https://sso.zhihu.com/sign-in?redirect=...",
        "sso.zhihu.com",
        "2025-06-03T10:04:00",
        false,
        1,
    );

    insert(
        conn,
        "zfs-reddit",
        "ZFS ARC cache tuning for NAS - reddit",
        "https://www.reddit.com/r/zfs/comments/abc123",
        "www.reddit.com",
        "2025-06-04T14:00:00",
        true,
        5,
    );
    insert(
        conn,
        "zfs-archwiki",
        "ZFS - ArchWiki - Performance Tuning",
        "https://wiki.archlinux.org/title/ZFS#Performance",
        "wiki.archlinux.org",
        "2025-06-04T14:15:00",
        true,
        4,
    );
    insert(
        conn,
        "zfs-truenas",
        "TrueNAS ZFS Pool Configuration Guide",
        "https://www.truenas.com/docs/zfs-pool-setup",
        "www.truenas.com",
        "2025-06-04T14:25:00",
        false,
        2,
    );

    insert(
        conn,
        "gha-devto",
        "Optimizing GitHub Actions with Smart Caching - DEV Community",
        "https://dev.to/user123/optimizing-github-actions-caching-abc",
        "dev.to",
        "2025-07-10T09:00:00",
        true,
        3,
    );
    insert(
        conn,
        "gha-docs",
        "Caching dependencies to speed up workflows - GitHub Docs",
        "https://docs.github.com/en/actions/using-workflows/caching-dependencies",
        "docs.github.com",
        "2025-07-10T09:20:00",
        false,
        2,
    );

    insert(
        conn,
        "disk-csdn",
        "Windows 磁盘占用100%的终极解决方案 - CSDN",
        "https://blog.csdn.net/user/article/disk-100-fix",
        "blog.csdn.net",
        "2025-08-15T16:00:00",
        false,
        4,
    );
    insert(
        conn,
        "disk-ms-support",
        "Fix high disk usage in Windows 10/11 - Microsoft Support",
        "https://support.microsoft.com/en-us/windows/disk-100-percent",
        "support.microsoft.com",
        "2025-08-15T16:15:00",
        false,
        2,
    );

    insert(
        conn,
        "openwrt-v2ex",
        "旁路由 DNS 分流设置教程 - V2EX",
        "https://www.v2ex.com/t/888888",
        "www.v2ex.com",
        "2025-09-01T20:00:00",
        true,
        3,
    );
    insert(
        conn,
        "openwrt-official",
        "OpenWrt DNSMasq Configuration Guide",
        "https://openwrt.org/docs/guide-user/base-system/dhcp.dnsmasq",
        "openwrt.org",
        "2025-09-01T20:20:00",
        false,
        2,
    );
    insert(
        conn,
        "openwrt-redirect",
        "Redirecting...",
        "https://openwrt.org/redirect?url=docs",
        "openwrt.org",
        "2025-09-01T20:01:00",
        false,
        1,
    );

    insert(
        conn,
        "rust-book-borrow",
        "References and Borrowing - The Rust Programming Language",
        "https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html",
        "doc.rust-lang.org",
        "2025-05-20T11:00:00",
        true,
        8,
    );
    insert(
        conn,
        "docker-tutorial",
        "Docker 入门到实践完全指南 - 掘金",
        "https://juejin.cn/post/docker-complete-guide",
        "juejin.cn",
        "2024-12-01T15:00:00",
        true,
        6,
    );

    // ── Adversarial distractors ──
    // Off-topic pages that share platform/structural words (知乎 / 题库 / 文档)
    // with the real targets. These mirror the actual noise observed on the live
    // DB for "驾考"; none of them belongs in any expected_top5. They exist to
    // punish over-broad expansion and the unicode61 literal-match failure.
    insert(
        conn,
        "noise-zhihu-xianyu",
        "闲鱼前车是啥意思啊？ - 知乎",
        "https://www.zhihu.com/question/700001",
        "www.zhihu.com",
        "2025-06-03T11:00:00",
        false,
        2,
    );
    insert(
        conn,
        "noise-zhihu-kaspersky",
        "卡巴斯基杀毒软件为什么不用清理就能用 - 知乎",
        "https://www.zhihu.com/question/700002",
        "www.zhihu.com",
        "2025-06-03T11:05:00",
        false,
        2,
    );
    insert(
        conn,
        "noise-zhihu-travel",
        "你有什么难忘的旅行经历吗？ - 知乎",
        "https://www.zhihu.com/question/700003",
        "www.zhihu.com",
        "2025-06-03T11:10:00",
        false,
        3,
    );
    insert(
        conn,
        "noise-apidoc-tiku",
        "言溪题库 文章 - 题库 - API 开发者文档",
        "https://api.example.com/docs/tiku",
        "api.example.com",
        "2025-06-03T11:15:00",
        false,
        1,
    );
    insert(
        conn,
        "noise-csdn-misc",
        "程序员如何提高效率 - CSDN 博客",
        "https://blog.csdn.net/user/article/efficiency",
        "blog.csdn.net",
        "2025-06-03T11:20:00",
        false,
        2,
    );
    insert(
        conn,
        "noise-zhihu-photo",
        "有哪些好看的风景图片推荐？ - 知乎",
        "https://www.zhihu.com/question/700004",
        "www.zhihu.com",
        "2025-06-03T11:25:00",
        false,
        2,
    );
    insert(
        conn,
        "noise-bili-vlog",
        "我的日常 vlog 视频合集 - bilibili",
        "https://www.bilibili.com/video/BV9999",
        "www.bilibili.com",
        "2025-06-03T11:30:00",
        false,
        1,
    );
}

#[test]
fn bench_recall_quality() {
    let conn = create_bench_db();
    create_bench_dataset(&conn);
    let segmenter = Segmenter::new();
    normalizer::normalize_all(&conn, &segmenter).unwrap();

    let mut hits_top1 = 0;
    let mut hits_top5 = 0;
    let total = BENCH_QUERIES.len();

    println!();
    println!("{}", "=".repeat(60));
    println!("  Recall Quality Benchmark ({} queries)", total);
    println!("{}", "=".repeat(60));
    println!();

    for query in BENCH_QUERIES {
        let results =
            search::search(&conn, &segmenter, None, query.query, None, None, None, 30).unwrap();
        let result_ids: Vec<&str> = results
            .iter()
            .map(|result| result.artifact.id.as_str())
            .collect();
        let top5 = &result_ids[..result_ids.len().min(5)];

        let hit1 = result_ids
            .first()
            .is_some_and(|id| query.expected_top5.contains(id));
        let hit5 = query.expected_top5.iter().any(|id| top5.contains(id));

        if hit1 {
            hits_top1 += 1;
        }
        if hit5 {
            hits_top5 += 1;
        }

        if query.query == "知乎 驾考" {
            assert!(
                !top5.contains(&"driving-sso"),
                "SSO/login noise should not appear in the top 5 for {:?}",
                query.query
            );
        }

        if query.query == "驾考" {
            // Pure-platform noise shares no content word with 驾考 (only stoplisted
            // platform words like 知乎/视频), so it must be fully excluded from the
            // top 5. (A page sharing a real topical word such as 题库 may legitimately
            // appear via the expanded layer, but only *below* the relevant results.)
            for noise in &[
                "noise-zhihu-xianyu",
                "noise-zhihu-kaspersky",
                "noise-zhihu-travel",
                "noise-zhihu-photo",
                "noise-bili-vlog",
            ] {
                assert!(
                    !top5.contains(noise),
                    "platform-word noise {:?} must not appear in top 5 for query \"驾考\" (got {:?})",
                    noise,
                    top5
                );
            }
            // The relevant driving pages must occupy the very top, ahead of any noise.
            assert!(
                hit1,
                "top-1 for \"驾考\" should be a relevant driving page, got {:?}",
                result_ids.first()
            );
        }

        let status = if hit5 { "hit" } else { "miss" };
        println!("  [{}] \"{}\"", status, query.query);
        println!("     scenario: {}", query.scenario);
        println!("     expected: {:?}", query.expected_top5);
        println!("     got top5: {:?}", top5);
        println!();
    }

    println!("{}", "=".repeat(60));
    println!(
        "  Top-1 Hit Rate: {}/{} ({:.0}%)",
        hits_top1,
        total,
        hits_top1 as f64 / total as f64 * 100.0
    );
    println!(
        "  Top-5 Hit Rate: {}/{} ({:.0}%)",
        hits_top5,
        total,
        hits_top5 as f64 / total as f64 * 100.0
    );
    println!("{}", "=".repeat(60));
    println!();

    assert!(
        hits_top5 as f64 / total as f64 >= 0.75,
        "Top-5 hit rate {:.0}% is below 75% baseline",
        hits_top5 as f64 / total as f64 * 100.0
    );
}
