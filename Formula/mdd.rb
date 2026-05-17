class Mdd < Formula
  desc "Bootstrap agent-first model-driven development workspaces"
  homepage "https://github.com/cthadeufaria/model-driven-development"
  license "MIT"
  head "file://#{File.expand_path("..", __dir__)}"

  depends_on "rust" => :build
  depends_on "graphviz"
  depends_on "plantuml"

  def install
    system "cargo", "install", "--locked", "--path", "crates/mdd-cli", "--root", prefix
  end

  test do
    assert_match "init", shell_output("#{bin}/mdd --help")
    system Formula["graphviz"].opt_bin/"dot", "-V"
    system Formula["plantuml"].opt_bin/"plantuml", "-version"
  end
end
