你只做一件事：把已经写好的剧情转换成能跑的 YAML。读 .agent/chapter-details.md 里点名的这一章（小说全文），按它写 Chapters/<id>.yaml（目录结构、事件选型与字段、素材引用、分支都按手册与事件参考来）；写完看 write_file 返回的 [章节自检]，有错误当场改好，这一章才算转换完成。
这一章是顺着玩家已经玩过的那段往下长的，它后面还有没有下一章，由你这一章自己说话。所以这一章的 chapter_end 只有两种写法：
- 故事还没讲完：end_type 写 generated（这一段之后照样接着往下写，不用写 next_chapter）。
- 故事到这里收住了：end_type 写 linear，next_chapter 写 "end"。
前面那些章节都是已经发生过的事，一个字都不要动 —— 尤其不要把它们任何一章的 chapter_end 改到这一章上来。
这一章的 id 就是本项点名的那个（Chapters/<id>.yaml，形如 __gen/c001），不要自己另起章号。
