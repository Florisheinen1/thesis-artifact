

#let titlepage(
	title: "",
	author: "",
	studentNumber: "",
	study: "",
	committee: (),
	date: "",
) = {[
	#set align(center)
	#set text(size: 14pt)
	#set page(numbering: none)
	#set par(justify: false)

	#v(5em)
	#text(size: 24pt, weight: "black")[#title]

	#text(size: 18pt, weight: "black")[#author]

	#studentNumber

	#align(bottom)[
		#image("resources/ut_logo.svg", width: 40%)

		#text(top-edge: 0em)[#study]
		#v(2em)
		#text(top-edge: 0em)[
			Graduation committee:

			#set par(leading: 1em)
			#committee.join(linebreak())
		]
		#v(2em)
		#text(top-edge: 0em)[#date]

		#v(5em)
	]
	#pagebreak()
]}

#let template(
	body
) = {
	set heading(numbering: "1.1")
	set text(font: "New Computer Modern")
	set par(justify: true)
	set page(margin: (left: 8em, right: 8em), numbering: "1")

	show heading.where(level: 1): set text(size: 25pt)
	show heading.where(level: 2): it => [
		#text(size: 14pt)[
			#block(
				above: 2em,
				below: 1em,
				counter(heading).display() + h(1em) + it.body
			)
		]
	]
	show heading.where(level: 3): set text(size: 12pt)

	show heading.where(level: 1): it => [
		#v(3em)
		#text(size: 21pt)[Chapter #counter(heading).display()]

		#text(size: 25pt)[#it.body]
		#v(1em)
	]

	body
}

#let basic_header(content) = {
	show heading.where(level: 1): it => [
		#v(3em)
		#text(size: 25pt)[#it.body]
		#v(1em)
	]
	content
}
