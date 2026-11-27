# u0 as main is now: the rolling `nightly` prerelease, replaced on each
# push by .github/workflows/app.yml.
cask "u0@nightly" do
  version :latest
  sha256 :no_check

  url "https://github.com/u0-code/u0/releases/download/nightly/u0-macos-arm64.zip"
  name "u0 Nightly"
  desc "Desktop app for the coding agents you already use"
  homepage "https://github.com/u0-code/u0"

  conflicts_with cask: "u0"
  depends_on arch: :arm64
  depends_on macos: :ventura

  app "u0.app"

  # u0 is signed ad hoc, not notarized, and Gatekeeper refuses to open
  # it while it is quarantined: installing this cask is the choice to open it.
  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/u0.app"],
                          writable_paths: ["u0.app"], writable_base: :appdir
  end

  zap trash: "~/.u0"

  caveats "u0 is not notarized; this cask removed the quarantine from u0.app so that it opens."
end
