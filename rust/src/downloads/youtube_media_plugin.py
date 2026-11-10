from yt_dlp.extractor.youtube import YoutubeIE

class _BetterPodMediaIE(YoutubeIE, plugin_name='betterpod_media'):
    # Keep audio-language inference, but do not expand unused subtitle URLs.
    _SUBTITLE_FORMATS = ()

    def _real_extract(self, url):
        info = super()._real_extract(url)
        # yt-dlp's subtitle table expects each present language to have formats.
        for field in ('subtitles', 'automatic_captions'):
            if isinstance(info.get(field), dict):
                info[field] = {lang: formats for lang, formats in info[field].items() if formats}
        return info
