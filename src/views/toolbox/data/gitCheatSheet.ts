/** Git 命令速查静态数据（GitCheatSheetTool 数据源）。 */

export interface CheatEntry {
  /** 场景描述。 */
  scenario: string;
  /** 命令（可复制）。 */
  command: string;
  /** 可选补充说明。 */
  note?: string;
}

export interface CheatGroup {
  title: string;
  entries: CheatEntry[];
}

export const GIT_GROUPS: CheatGroup[] = [
  {
    title: "撤销与回滚",
    entries: [
      { scenario: "撤销某个文件的工作区修改", command: "git restore <file>" },
      { scenario: "撤销所有工作区修改", command: "git restore .", note: "不可恢复，慎用" },
      { scenario: "取消暂存（保留改动）", command: "git restore --staged <file>" },
      { scenario: "修正最后一次提交", command: "git commit --amend", note: "可改 message 或补漏提交的文件" },
      { scenario: "回退到某提交，改动留工作区", command: "git reset --soft <commit>" },
      { scenario: "回退到某提交，改动留未暂存", command: "git reset --mixed <commit>" },
      { scenario: "回退到某提交，丢弃全部改动", command: "git reset --hard <commit>", note: "危险：改动直接丢失" },
      { scenario: "安全反转某次提交（生成反向新提交）", command: "git revert <commit>", note: "已推送的提交用 revert，不要 reset" },
      { scenario: "找回丢失的提交/分支", command: "git reflog", note: "找到 hash 后 git reset --hard 或建分支" },
    ],
  },
  {
    title: "分支",
    entries: [
      { scenario: "新建并切换分支", command: "git switch -c <name>" },
      { scenario: "切换分支", command: "git switch <name>" },
      { scenario: "基于远程分支建本地分支", command: "git switch -c <name> origin/<name>" },
      { scenario: "重命名当前分支", command: "git branch -m <new-name>" },
      { scenario: "删除已合并的本地分支", command: "git branch -d <name>" },
      { scenario: "强制删除本地分支", command: "git branch -D <name>", note: "未合并的改动会丢" },
      { scenario: "删除远程分支", command: "git push origin --delete <name>" },
    ],
  },
  {
    title: "暂存（stash）",
    entries: [
      { scenario: "暂存当前改动", command: "git stash" },
      { scenario: "含未跟踪文件一起暂存", command: "git stash -u" },
      { scenario: "暂存并写备注", command: "git stash push -m \"说明\"" },
      { scenario: "查看暂存列表", command: "git stash list" },
      { scenario: "恢复最近一次暂存并删除记录", command: "git stash pop" },
      { scenario: "应用指定暂存（保留记录）", command: "git stash apply stash@{0}" },
      { scenario: "删除指定暂存", command: "git stash drop stash@{0}" },
    ],
  },
  {
    title: "远程同步",
    entries: [
      { scenario: "拉取并变基本地提交", command: "git pull --rebase", note: "保持线性历史" },
      { scenario: "推送并关联上游分支", command: "git push -u origin <branch>" },
      { scenario: "安全强推（远端被他人更新时失败）", command: "git push --force-with-lease" },
      { scenario: "同步远程分支列表（清理已删分支）", command: "git fetch --prune" },
      { scenario: "查看远程仓库地址", command: "git remote -v" },
    ],
  },
  {
    title: "变基",
    entries: [
      { scenario: "交互式整理最近 n 个提交", command: "git rebase -i HEAD~3", note: "squash / reword / drop" },
      { scenario: "变基到主干最新", command: "git rebase origin/master" },
      { scenario: "解决冲突后继续", command: "git rebase --continue" },
      { scenario: "放弃本次变基", command: "git rebase --abort" },
    ],
  },
  {
    title: "日志与排查",
    entries: [
      { scenario: "图形化简洁日志（全分支）", command: "git log --oneline --graph --all" },
      { scenario: "查看某文件的逐提交改动", command: "git log -p <file>" },
      { scenario: "查看文件每行最后是谁改的", command: "git blame <file>" },
      { scenario: "按内容搜索提交", command: "git log -S \"关键字\" --oneline" },
      { scenario: "二分定位引入 bug 的提交", command: "git bisect start", note: "good/bad 标记后自动二分" },
    ],
  },
  {
    title: "标签",
    entries: [
      { scenario: "打轻量标签", command: "git tag v1.0.0" },
      { scenario: "打附注标签（推荐发版用）", command: "git tag -a v1.0.0 -m \"发布说明\"" },
      { scenario: "推送全部标签", command: "git push origin --tags" },
      { scenario: "删除远程标签", command: "git push origin :refs/tags/v1.0.0" },
    ],
  },
];
