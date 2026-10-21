class Things < Formula
  desc "AI-first Things 3 CLI and MCP server"
  homepage "https://github.com/guangzan/things"
  url "https://github.com/guangzan/things/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_RELEASE_SHA256"
  license "MIT"
  head "https://github.com/guangzan/things.git", branch: "main"

  depends_on :macos
  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/things-cli")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/things --version")
    assert_match "Things", shell_output("#{bin}/things --help")
  end
end
