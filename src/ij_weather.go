package main

// ij_weather.go — Games → Weather: the forecast for the places you choose
// (there is no location, so you search for a place, with the keyboard).
//
//	Weather
//	  ⌕ Search for a Place
//	  ☀ Milan                         18°
//	    Lombardy, Italy · Partly Cloudy
//	  ☂ London                        11°
//	  Units                           °C
//
// The data comes from Open-Meteo (open-meteo.com: free, no key): the
// geocoding API to find places and the forecast API for the current
// weather and 10 days. Places and their last forecast are kept in
// weather.json in the data folder, so the list and the forecast show at
// once (and offline), and are refreshed when older than 30 minutes.

import (
	"encoding/json"
	"fmt"
	"image"
	"io"
	"math"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"
)

func meteoGeoBase() string {
	if b := os.Getenv("MIYOOPOD_METEO_GEO"); b != "" {
		return strings.TrimRight(b, "/")
	}
	return "https://geocoding-api.open-meteo.com"
}

func meteoAPIBase() string {
	if b := os.Getenv("MIYOOPOD_METEO_API"); b != "" {
		return strings.TrimRight(b, "/")
	}
	return "https://api.open-meteo.com"
}

func meteoGet(u string, v any) error {
	req, _ := http.NewRequest("GET", u, nil)
	req.Header.Set("User-Agent", "KnulliPod/"+KNULLIPOD_VERSION)
	resp, err := updateClient(20 * time.Second).Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return fmt.Errorf("Open-Meteo answered %d", resp.StatusCode)
	}
	return json.NewDecoder(io.LimitReader(resp.Body, 4<<20)).Decode(v)
}

// ---- data ----

// wxPlace is a saved place and its last forecast.
type wxPlace struct {
	Name     string      `json:"name"`
	Admin    string      `json:"admin,omitempty"`   // region
	Country  string      `json:"country,omitempty"` // in full
	Lat      float64     `json:"lat"`
	Lon      float64     `json:"lon"`
	Forecast *wxForecast `json:"forecast,omitempty"`

	thumb *Album // stand-in album for the list's icon
}

func (p *wxPlace) where() string {
	var parts []string
	for _, s := range []string{p.Admin, p.Country} {
		if s != "" && s != p.Name {
			parts = append(parts, s)
		}
	}
	return strings.Join(parts, ", ")
}

// wxForecast is Open-Meteo's answer, as it comes (metric).
type wxForecast struct {
	Fetched   time.Time `json:"fetched"`
	UTCOffset int       `json:"utc_offset_seconds"`
	Timezone  string    `json:"timezone"`
	Current   struct {
		Time     string  `json:"time"`
		Temp     float64 `json:"temperature_2m"`
		Feels    float64 `json:"apparent_temperature"`
		Humidity float64 `json:"relative_humidity_2m"`
		IsDay    int     `json:"is_day"`
		Code     int     `json:"weather_code"`
		Wind     float64 `json:"wind_speed_10m"`
		WindDir  float64 `json:"wind_direction_10m"`
	} `json:"current"`
	Daily struct {
		Time    []string  `json:"time"`
		Code    []int     `json:"weather_code"`
		Max     []float64 `json:"temperature_2m_max"`
		Min     []float64 `json:"temperature_2m_min"`
		Rain    []float64 `json:"precipitation_probability_max"`
		Sunrise []string  `json:"sunrise"`
		Sunset  []string  `json:"sunset"`
	} `json:"daily"`
}

// days: how many complete days the forecast has.
func (f *wxForecast) days() int {
	d := &f.Daily
	n := len(d.Time)
	for _, l := range []int{len(d.Code), len(d.Max), len(d.Min)} {
		n = min(n, l)
	}
	return n
}

func (f *wxForecast) rain(i int) float64 {
	if i < len(f.Daily.Rain) {
		return f.Daily.Rain[i]
	}
	return -1
}

// localNow: the time at the place.
func (f *wxForecast) localNow() time.Time {
	return time.Now().UTC().Add(time.Duration(f.UTCOffset) * time.Second)
}

func fetchForecast(p *wxPlace) (*wxForecast, error) {
	v := url.Values{}
	v.Set("latitude", fmt.Sprintf("%.4f", p.Lat))
	v.Set("longitude", fmt.Sprintf("%.4f", p.Lon))
	v.Set("timezone", "auto")
	v.Set("current", "temperature_2m,relative_humidity_2m,apparent_temperature,is_day,weather_code,wind_speed_10m,wind_direction_10m")
	v.Set("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset")
	v.Set("forecast_days", "10")
	var f wxForecast
	if err := meteoGet(meteoAPIBase()+"/v1/forecast?"+v.Encode(), &f); err != nil {
		return nil, err
	}
	if f.days() == 0 {
		return nil, fmt.Errorf("Open-Meteo sent no forecast")
	}
	f.Fetched = time.Now()
	return &f, nil
}

type wxGeoResult struct {
	Name    string  `json:"name"`
	Admin1  string  `json:"admin1"`
	Country string  `json:"country"`
	Lat     float64 `json:"latitude"`
	Lon     float64 `json:"longitude"`
	Pop     int     `json:"population"`
	Feature string  `json:"feature_code"`
}

func searchPlaces(q string) ([]wxGeoResult, error) {
	v := url.Values{}
	v.Set("name", q)
	v.Set("count", "20")
	v.Set("language", "en")
	v.Set("format", "json")
	var out struct {
		Results []wxGeoResult `json:"results"`
	}
	if err := meteoGet(meteoGeoBase()+"/v1/search?"+v.Encode(), &out); err != nil {
		return nil, err
	}
	return out.Results, nil
}

// ---- saved state ----

type wxState struct {
	Fahrenheit bool       `json:"fahrenheit,omitempty"`
	Places     []*wxPlace `json:"places"`
}

var wxSaved *wxState

func wxPath() string { return dataPath("weather.json") }

func wxLoad() *wxState {
	if wxSaved != nil {
		return wxSaved
	}
	wxSaved = &wxState{}
	if data, err := os.ReadFile(wxPath()); err == nil {
		_ = json.Unmarshal(data, wxSaved)
	}
	// a saved forecast without days (an old or partial file) is dropped,
	// so the screens can rely on day 0
	for _, p := range wxSaved.Places {
		if p.Forecast != nil && p.Forecast.days() == 0 {
			p.Forecast = nil
		}
	}
	return wxSaved
}

func wxSave() {
	if wxSaved == nil {
		return
	}
	data, err := json.MarshalIndent(wxSaved, "", " ")
	if err == nil {
		err = writeFileAtomic(wxPath(), data)
	}
	if err != nil {
		logMsg("Weather: saving: " + err.Error())
	}
}

// ---- units ----

func wxTemp(c float64) string {
	if wxLoad().Fahrenheit {
		c = c*9/5 + 32
	}
	return fmt.Sprintf("%d°", int(math.Round(c)))
}

func wxWind(kmh float64) string {
	if wxLoad().Fahrenheit {
		return fmt.Sprintf("%d mph", int(math.Round(kmh*0.621371)))
	}
	return fmt.Sprintf("%d km/h", int(math.Round(kmh)))
}

func compass(deg float64) string {
	dirs := []string{"N", "NE", "E", "SE", "S", "SW", "W", "NW"}
	return dirs[int(math.Round(math.Mod(deg+360, 360)/45))%8]
}

// wxStale: a forecast older than this is fetched again.
const wxStale = 30 * time.Minute

func agoText(t time.Time) string {
	d := time.Since(t)
	switch {
	case d < time.Minute:
		return "just now"
	case d < time.Hour:
		m := int(d.Minutes())
		return fmt.Sprintf("%d %s ago", m, pluralize(m, "minute", "minutes"))
	case d < 48*time.Hour:
		h := int(d.Hours())
		return fmt.Sprintf("%d %s ago", h, pluralize(h, "hour", "hours"))
	}
	n := int(d.Hours() / 24)
	return fmt.Sprintf("%d days ago", n)
}

// placeThumb: the list's icon for the place (its weather now).
func placeThumb(p *wxPlace) *Album {
	kind := wxCloudy
	if f := p.Forecast; f != nil {
		kind = wxKindFor(f.Current.Code, f.Current.IsDay == 1)
	}
	if p.thumb == nil || p.thumb.Name != fmt.Sprint(kind) {
		img := wxIcon(kind, 96, -1)
		p.thumb = &Album{Name: fmt.Sprint(kind), ArtImg: image.Image(img)}
	}
	return p.thumb
}

// ---- the list ----

// openWeather: the places, the search and the units.
func (ui *IpodUI) openWeather() {
	st := wxLoad()
	s := &struct {
		term, ran, status string
		busy              bool
		res               []wxGeoResult
		gen               int
	}{}
	v := &View{ID: "weather", Title: "Weather", Kind: KindFull}
	var run func()
	add := func(r wxGeoResult) {
		p := &wxPlace{Name: r.Name, Admin: r.Admin1, Country: r.Country, Lat: r.Lat, Lon: r.Lon}
		// already there? open it
		for _, q := range st.Places {
			if math.Abs(q.Lat-p.Lat) < 0.01 && math.Abs(q.Lon-p.Lon) < 0.01 {
				p = q
				goto open
			}
		}
		st.Places = append([]*wxPlace{p}, st.Places...)
		wxSave()
	open:
		s.res, s.ran, s.status, s.term = nil, "", "", ""
		ui.refreshView(v)
		ui.showView(ui.forecastView(p), "")
	}
	v.Build = func() []Item {
		editing := ui.keyboardEditing(&s.term)
		label, sub := "Search for a Place", "A city or a town"
		switch {
		case editing:
			sub = ui.withCaret(&s.term)
			if s.term == "" {
				sub = "Type a place, then ✓ to search"
			}
		case s.ran != "" && !s.busy:
			label = "Edit search"
			sub = fmt.Sprintf("%s · %d %s", s.ran, len(s.res), pluralize(len(s.res), "place", "places"))
		}
		edit := func() {
			ui.showKeyboard(&s.term, func() { ui.refreshView(v) }, run)
			ui.refreshView(v)
		}
		items := []Item{{Label: label, Sublabel: sub, Icon: "search_icon", OnSelect: edit}}
		switch {
		case s.busy:
			items = append(items, Item{Label: "Searching…", NoArrow: true})
		case s.status != "":
			l, sb, _ := strings.Cut(s.status, " · ")
			items = append(items, Item{Label: l, Sublabel: sb, NoArrow: true, OnSelect: func() {
				if strings.HasSuffix(s.status, "press to retry") {
					run()
				}
			}})
		}
		for _, r := range s.res {
			r := r
			p := &wxPlace{Name: r.Name, Admin: r.Admin1, Country: r.Country}
			items = append(items, Item{Label: r.Name, Sublabel: p.where(), OnSelect: func() { add(r) }})
		}
		if len(s.res) > 0 {
			return items // choosing a place comes first
		}
		for _, p := range st.Places {
			p := p
			it := Item{Label: p.Name, Sublabel: p.where(), Album: placeThumb(p),
				OnSelect: func() { ui.showView(ui.forecastView(p), "") },
				LongPress: []SheetOption{{Label: "Remove " + p.Name, Danger: true, OnSelect: func() {
					for i, q := range st.Places {
						if q == p {
							st.Places = append(st.Places[:i], st.Places[i+1:]...)
							break
						}
					}
					wxSave()
					ui.refreshView(v)
				}}}}
			if f := p.Forecast; f != nil {
				it.Value = wxTemp(f.Current.Temp)
				it.Sublabel = strings.Trim(it.Sublabel+" · "+wxLabel(f.Current.Code), " ·")
			}
			items = append(items, it)
		}
		unit := "°C"
		if st.Fahrenheit {
			unit = "°F"
		}
		items = append(items, Item{Label: "Units", Value: unit, Sublabel: "Weather data by Open-Meteo.com", NoArrow: true, OnSelect: func() {
			st.Fahrenheit = !st.Fahrenheit
			wxSave()
			ui.refreshView(v)
		}})
		return items
	}
	run = func() {
		q := strings.TrimSpace(s.term)
		if q == "" {
			return
		}
		s.gen++
		gen := s.gen
		s.busy, s.status, s.ran, s.res = true, "", q, nil
		v.Index = 0
		ui.refreshView(v)
		go func() {
			res, err := searchPlaces(q)
			ui.postSure(func() {
				if gen != s.gen {
					return
				}
				s.busy = false
				switch {
				case err != nil:
					s.status = "Couldn't search · " + iaErrText(err) + " · press to retry"
				case len(res) == 0:
					s.status = "No places found · try another name"
				}
				s.res = res
				ui.refreshView(v)
				if len(res) > 0 {
					v.Index = 1
				}
			})
		}()
	}
	v.Items = v.Build()
	if len(st.Places) > 0 {
		v.Index = 1
	}
	ui.showView(v, "")
	if len(st.Places) == 0 {
		ui.showKeyboard(&s.term, func() { ui.refreshView(v) }, run)
		ui.refreshView(v)
	}
}
