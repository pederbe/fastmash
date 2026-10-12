class Fastmash < Formula
  desc "Fast command-line statistics and table transformations"
  homepage "https://fastmash.io"
  url "https://github.com/pederbe/fastmash/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "e331d2d9499dc502f16e253ab4c408ceed4cca251420c53718ea8f70595e0302"
  license any_of: ["MIT", "Apache-2.0"]

  depends_on "python@3.14" => :build
  depends_on "rust" => :build

  on_macos do
    depends_on arch: :arm64
    depends_on macos: :sequoia
  end

  on_linux do
    depends_on arch: :x86_64
  end

  def install
    if OS.mac?
      # The Sort supervisor is Linux-only; macOS sorts with the built-in sorter.
      system "cargo", "install", "--bin", "fastmash", *std_cargo_args(path: "crates/cli")
      target = "aarch64-apple-darwin"
    else
      system "cargo", "install", *std_cargo_args(path: "crates/cli")
      target = "x86_64-unknown-linux-gnu"
    end
    notices = Utils.safe_popen_read("python3", "scripts/third_party_licenses.py", "--target", target)
    (buildpath/"THIRD-PARTY-LICENSES.md").write notices
    doc.install "README.md", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"
  end

  test do
    %w[LICENSE-MIT LICENSE-APACHE THIRD-PARTY-LICENSES.md].each do |document|
      assert_predicate doc/document, :size?
    end
    assert_equal "fastmash #{version}\n", shell_output("#{bin}/fastmash --version")
    assert_equal "6\t2\n", pipe_output("#{bin}/fastmash sum 1 mean 1", "1\n2\n3\n", 0)
    assert_equal "1.8171205928321\n", pipe_output("#{bin}/fastmash geomean 1", "1\n2\n3\n", 0)

    # A paired operation with a route trace shows which sorter ran.
    (testpath/"input").write "b\t2\t3\na\t1\t2\na\t3\t6\n"
    assert_equal "a\t2\nb\t0\n",
                 shell_output("LC_ALL=C FASTMASH_GROUPING=sort FASTMASH_SORT_TRACE=1 " \
                              "#{bin}/fastmash -sg1 pcov 2:3 <input 2>trace")
    if OS.mac?
      refute_path_exists bin/"fastmash-sort-supervisor"
      assert_match(/\Asort route: native sort/, (testpath/"trace").read)
    else
      # An unusable Sort supervisor would fall back to the built-in sorter.
      assert_path_exists bin/"fastmash-sort-supervisor"
      assert_equal "sort route: system sort\n", (testpath/"trace").read
    end
  end
end
