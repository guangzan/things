#!/usr/bin/env ruby
# frozen_string_literal: true

formula_path = ARGV.fetch(0, "Formula/things.rb")
expected_version = ARGV[1]
content = File.read(formula_path)

unless content.match?(/^class Things < Formula$/)
  abort "#{formula_path}: expected `class Things < Formula`"
end

if content.match?(/^  test do$/)
  abort "#{formula_path}: Formula already has a test block"
end

# Homebrew can infer the stable version from the release URL. Keeping an
# explicit version makes `brew audit --strict` fail for cargo-dist formulas.
content = content.sub(/^  version "[^"]+"\n/, "")

unless content.match?(/^  test do$/)
  test_block = <<~'RUBY'

      test do
        assert_match version.to_s, shell_output("#{bin}/things --version")
      end
    end
  RUBY
  content = content.sub(/^end\s*\z/, test_block)
end

unless content.match?(/^  test do$/)
  abort "#{formula_path}: failed to add test block"
end

if expected_version && !content.include?("/download/v#{expected_version}/")
  abort "#{formula_path}: release URLs do not match version #{expected_version}"
end

File.write(formula_path, content)
