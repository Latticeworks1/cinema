"""Programmatic access to cinema: make a project from a recording, shape it, and render it.

    import cinema
    p = cinema.Project.create("song.mp3")        # any file name, any format ffmpeg reads
    p.prepare()                                  # transcribe, analyse
    p.set("direction.fit", 0.9)                  # checked by planning with the edit
    p.render(out="song.mp4", small=True)
"""

from .project import CinemaError, Project, slug

__all__ = ["CinemaError", "Project", "slug"]
