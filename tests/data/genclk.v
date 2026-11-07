module g(clka,noclk,din,o1,o2,o3);
input clka,noclk,din; output o1,o2,o3;
wire gq,nd,gb,nx;
DFF rdiv(.CK(clka),.D(nd),.Q(gq));
INV udiv(.A(gq),.Y(nd));
BUF ub(.A(gq),.Y(gb));
BUF ux(.A(noclk),.Y(nx));
DFF r1(.CK(gq),.D(din),.Q(o1));
DFF r2(.CK(gb),.D(din),.Q(o2));
DFF r3(.CK(nx),.D(din),.Q(o3));
endmodule
