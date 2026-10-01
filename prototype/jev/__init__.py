"""Jev — route a question to the model that should answer it.

Jev is a decision layer. It does not answer questions; it reads a transcript,
decides which capability the question needs, and resolves that capability to a
configured target (a local model, a cloud API, or an external harness).
"""

__version__ = "0.1.0"
